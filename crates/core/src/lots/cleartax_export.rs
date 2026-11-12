//! ClearTax capital gains workbook (template v7).
//!
//! Produces the "Sale of Shares" and "Sale of Mutual Funds" sheets ClearTax
//! accepts for upload, with the same columns, number formats and
//! auto-calculated columns as the template. ClearTax applies grandfathering
//! itself from the FMV columns, so the purchase value written here is the
//! actual cost, not the deemed cost.
//!
//! ClearTax accepts at most 5,000 rows per file, so a large year is split into
//! numbered parts.

use chrono::{Datelike, NaiveDate};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_xlsxwriter::{DataValidation, ExcelDateTime, Format, Formula, Workbook, Worksheet};

use super::{IndianAssetTaxClassification, IndianCapitalGainCalculation, IndianTaxYearSummary};
use crate::errors::{Error, Result};

/// Most rows ClearTax accepts in one sheet of one file.
pub const CLEARTAX_MAX_ROWS: usize = 5000;

pub const MF_EQUITY: &str = "MF (Equity)";
pub const MF_OTHER: &str = "MF (Other than Equity)";

/// Where a sale goes in the ClearTax workbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearTaxSheet {
    Shares,
    MutualFund { equity: bool },
}

/// The sheet a sale belongs on, or `None` for assets the ClearTax upload does
/// not cover (gold, bonds, debentures, other assets). Those have to be
/// entered in ClearTax by hand.
pub fn cleartax_sheet(calc: &IndianCapitalGainCalculation) -> Option<ClearTaxSheet> {
    match calc.classification {
        IndianAssetTaxClassification::EquityListed | IndianAssetTaxClassification::ReitInvIt => {
            Some(ClearTaxSheet::Shares)
        }
        IndianAssetTaxClassification::EquityMf => Some(ClearTaxSheet::MutualFund { equity: true }),
        IndianAssetTaxClassification::DebtMf
        | IndianAssetTaxClassification::NonEquityEtf
        | IndianAssetTaxClassification::NonEquityFund => {
            Some(ClearTaxSheet::MutualFund { equity: false })
        }
        IndianAssetTaxClassification::UnlistedDebenturePost2024
        | IndianAssetTaxClassification::OtherCapitalAsset
        | IndianAssetTaxClassification::GoldSgb => None,
    }
}

struct Rows<'a> {
    shares: Vec<&'a IndianCapitalGainCalculation>,
    funds: Vec<(&'a IndianCapitalGainCalculation, bool)>,
}

fn split_rows(summary: &IndianTaxYearSummary) -> Rows<'_> {
    let mut calcs: Vec<&IndianCapitalGainCalculation> = summary.calculations.iter().collect();
    calcs.sort_by(|a, b| {
        (a.disposal_date, &a.asset_name, a.acquisition_date).cmp(&(
            b.disposal_date,
            &b.asset_name,
            b.acquisition_date,
        ))
    });
    let mut rows = Rows {
        shares: Vec::new(),
        funds: Vec::new(),
    };
    for calc in calcs {
        match cleartax_sheet(calc) {
            Some(ClearTaxSheet::Shares) => rows.shares.push(calc),
            Some(ClearTaxSheet::MutualFund { equity }) => rows.funds.push((calc, equity)),
            None => {}
        }
    }
    rows
}

/// Number of ClearTax files the year needs: 0 when nothing goes in the
/// workbook, otherwise enough to keep each sheet within 5,000 rows.
pub fn cleartax_file_count(summary: &IndianTaxYearSummary) -> usize {
    let rows = split_rows(summary);
    rows.shares
        .len()
        .max(rows.funds.len())
        .div_ceil(CLEARTAX_MAX_ROWS)
}

/// Number of sales left out of the ClearTax workbook.
pub fn cleartax_excluded_count(summary: &IndianTaxYearSummary) -> usize {
    summary
        .calculations
        .iter()
        .filter(|c| cleartax_sheet(c).is_none())
        .count()
}

/// File name for one part, e.g. `cleartax_capital_gains_fy2024-25.xlsx` or
/// `cleartax_capital_gains_fy2024-25_part2of3.xlsx`.
pub fn cleartax_file_name(fy_start_year: i32, part: usize, parts: usize) -> String {
    let fy = format!("fy{}-{:02}", fy_start_year, (fy_start_year + 1) % 100);
    if parts > 1 {
        format!("cleartax_capital_gains_{fy}_part{}of{parts}.xlsx", part + 1)
    } else {
        format!("cleartax_capital_gains_{fy}.xlsx")
    }
}

fn xlsx_err(e: rust_xlsxwriter::XlsxError) -> Error {
    Error::Unexpected(format!("Could not build the ClearTax workbook: {e}"))
}

fn num(d: Decimal) -> f64 {
    d.to_f64().unwrap_or_default()
}

fn excel_date(d: NaiveDate) -> Result<ExcelDateTime> {
    let year = u16::try_from(d.year())
        .map_err(|_| Error::Unexpected(format!("Date {d} is outside Excel's range")))?;
    // month() and day() are at most 12 and 31.
    ExcelDateTime::from_ymd(year, d.month() as u8, d.day() as u8).map_err(xlsx_err)
}

fn grandfathering_cutoff() -> NaiveDate {
    NaiveDate::from_ymd_opt(2018, 1, 31).expect("hard-coded date is valid")
}

/// FMV per unit, given only for units bought on or before 31 Jan 2018, as the
/// template asks.
fn fmv_for(calc: &IndianCapitalGainCalculation) -> Option<Decimal> {
    if calc.acquisition_date <= grandfathering_cutoff() {
        calc.fmv_per_unit_2018
    } else {
        None
    }
}

/// The template's "Net capital gain (auto-calculated)" formula, evaluated so
/// the file shows the right value before Excel recalculates.
fn template_net_gain(calc: &IndianCapitalGainCalculation, grandfather: bool) -> Decimal {
    let sale_value = calc.quantity * calc.sale_price_per_unit;
    let cost = calc.actual_cost;
    let expenses = calc.transfer_expenses;
    let held_days = (calc.disposal_date - calc.acquisition_date).num_days();
    if grandfather && calc.acquisition_date <= grandfathering_cutoff() && held_days > 365 {
        let fmv_total = calc.quantity * fmv_for(calc).unwrap_or_default();
        sale_value - expenses - cost.max(fmv_total.min(sale_value))
    } else {
        sale_value - cost - expenses
    }
}

struct Formats {
    header: Format,
    qty: Format,
    date: Format,
    whole: Format,
    money: Format,
    wrap: Format,
    title: Format,
}

impl Formats {
    fn new() -> Self {
        // Number formats copied from the ClearTax template.
        let four = "_ * #,##0.0000_ ;_ * \\-#,##0.0000_ ;_ * \"-\"??_ ;_ @_ ";
        let whole = "_ * #,##0_ ;_ * \\-#,##0_ ;_ * \"-\"??_ ;_ @_ ";
        let two = "_ * #,##0.00_ ;_ * \\-#,##0.00_ ;_ * \"-\"??_ ;_ @_ ";
        Formats {
            header: Format::new().set_bold().set_text_wrap(),
            qty: Format::new().set_num_format(four),
            date: Format::new().set_num_format("[$-14009]d\\ mmm\\ yyyy"),
            whole: Format::new().set_num_format(whole),
            money: Format::new().set_num_format(two),
            wrap: Format::new().set_text_wrap(),
            title: Format::new().set_bold(),
        }
    }
}

fn write_headers(ws: &mut Worksheet, headers: &[&str], f: &Formats) -> Result<()> {
    for (col, h) in headers.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *h, &f.header)
            .map_err(xlsx_err)?;
        ws.set_column_width(col as u16, 18).map_err(xlsx_err)?;
    }
    ws.set_freeze_panes(1, 0).map_err(xlsx_err)?;
    Ok(())
}

const SHARES_HEADERS: [&str; 12] = [
    "ISIN",
    "Description of shares sold",
    "Number of Shares",
    "Date of Purchase\n(DD/MM/YYYY)",
    "Total Purchase Value",
    "Date of Sale\n(DD/MM/YYYY)",
    "Sale Price per Share",
    "Total Sale Value",
    "FMV per Share as on\n31 Jan. 2018",
    "FMV as on\n31 Jan. 2018",
    "Transfer Expenses\n(Stamp duty, brokerage, etc)",
    "Net capital gain\n(auto-calculated)",
];

const FUNDS_HEADERS: [&str; 13] = [
    "Type of Mutual Fund (MF)",
    "ISIN\n(only if the type is \"Equity\")",
    "Description of Mutual Fund sold",
    "Number of Units",
    "Date of Purchase\n(DD/MM/YYYY)",
    "Total Purchase Value",
    "Date of Sale\n(DD/MM/YYYY)",
    "Sale Price per Unit",
    "Total Sale Value",
    "FMV per Unit as on\n31 Jan. 2018",
    "FMV as on\n31 Jan. 2018",
    "Transfer Expenses\n(Stamp duty, brokerage, etc)",
    "Net capital gain\n(auto-calculated)",
];

const FNO_HEADERS: [&str; 13] = [
    "Asset Type",
    "Description",
    "Buy Date",
    "Sell Date",
    "Qty",
    "Buy Rate",
    "Buy Amount",
    "Sell Rate",
    "Sell Amount",
    "Gross Profit / Loss",
    "Expenses\n(brokerage, stamp duty, STT etc)",
    "Net Profit",
    "Turnover",
];

const INSTRUCTIONS: [(u32, &str); 10] = [
    (1, "\"Sale of Shares / Mutual Funds\""),
    (3, "In this sheet, you should enter details of any sales you have made in the relevant financial year.\n\nPlease note that if you have only purchased some Shares / Mutual Funds, you do not need to report it in your tax return. Capital gain only applies when you sell the Shares / Mutual Funds you had purchased."),
    (5, "Maximum of 5000 line items are allowed in single file, in case of more than 5000 line items kindly split the data into multiple files and upload it."),
    (7, "You need to enter the following fields:"),
    (9, "ISIN:\n\nISIN is an unique 12 digit alphanumeric code issued by SEBI to each securities listed on stock exchange.\nISIN is mandatory  only in case of sale of long term listed shares and equity oriented mutual funds (period of holding is more than 12 months)"),
    (11, "Sale price per share and FMV per share:\n\nIt is mandatory only in case of sale of long term listed shares and equity oriented mutual funds (period of holding is more than 12 months)"),
    (13, "Purchase Date and Purchase Value :\n\nClearTax will use the purchase date to determine if the Shares / Mutual Funds sold can be classified as long term or short term. The purchase value is the cost you had paid to originally purchase the Shares / Mutual Funds you have sold.\n\nPlease enter the purchase value of the units you sold in relevant year. For example, if you had purchased 100 units of Infosys in 2010, and sold 30 units in 2018, the purchase value you need to report is the value of those 30 units in 2010 only."),
    (15, "FMV as on 31/01/2018\n\nFair market value of the asset as on 31st January 2018, required to derive cost of acquisition for the purpose of calculating long-term capital gains for listed shares or equity oriented mutual funds. It is mandatory to enter only if the date of purchase is on or before 31st Jan 2018."),
    (17, "Sale Date, Sale Value & Transfer Expenses :\n\nThe sale date (along with the purchase date) is used to determine if the Shares / Mutual Funds that you had sold can be classified as long term or short term. The sale date is also used in order to determine if there was any advance tax requirement.\n\nTransfer expenses from your broker (like brokerage fees, stamp duty fees, etc.) can be mentioned in the sheet. Your final capital gain is (Sale Value - maximum (Purchase Value, minimum(FMV, sale value)) - Transfer Expenses).\n\nPlease note that any STT (securities transfer tax) payment made by the broker cannot be claimed as transfer expenses."),
    (20, "Additional notes :\n\nThis sheet can only be used for sale of Listed Shares / Mutual Funds where STT was paid. If you had sold any Shares / Mutual Funds listed on BSE or NSE, this is most likely to be true. This sheet should not be used in case you have sold unlisted Shares / Mutual Funds.\n\nIf your Shares / Mutual Funds were purchased before 1st April 2004, the purchase transaction would not have STT applied. You should not use this sheet in that case, and use ClearTax's online interface."),
];

/// Writes the purchase/sale/FMV/expense columns shared by both sheets,
/// starting at column `c0` (the template's "Number of" column).
fn write_trade_columns(
    ws: &mut Worksheet,
    row: u32,
    c0: u16,
    calc: &IndianCapitalGainCalculation,
    f: &Formats,
) -> Result<()> {
    // Column letters used by the formulas: quantity, sale price, FMV per unit.
    // c0 + 6 is at most 9, so these are single letters.
    let letter = |offset: u16| char::from(b'A' + (c0 + offset) as u8);
    let (qty, price, fmv) = (letter(0), letter(4), letter(6));
    let r = row + 1; // 1-based row in formulas

    ws.write_number_with_format(row, c0, num(calc.quantity), &f.qty)
        .map_err(xlsx_err)?;
    ws.write_datetime_with_format(row, c0 + 1, excel_date(calc.acquisition_date)?, &f.date)
        .map_err(xlsx_err)?;
    ws.write_number_with_format(row, c0 + 2, num(calc.actual_cost), &f.qty)
        .map_err(xlsx_err)?;
    ws.write_datetime_with_format(row, c0 + 3, excel_date(calc.disposal_date)?, &f.date)
        .map_err(xlsx_err)?;
    ws.write_number_with_format(row, c0 + 4, num(calc.sale_price_per_unit), &f.qty)
        .map_err(xlsx_err)?;
    let sale_value = calc.quantity * calc.sale_price_per_unit;
    ws.write_formula_with_format(
        row,
        c0 + 5,
        Formula::new(format!("=({qty}{r}*{price}{r})")).set_result(num(sale_value).to_string()),
        &f.whole,
    )
    .map_err(xlsx_err)?;
    let fmv_unit = fmv_for(calc);
    if let Some(v) = fmv_unit {
        ws.write_number_with_format(row, c0 + 6, num(v), &f.qty)
            .map_err(xlsx_err)?;
    }
    ws.write_formula_with_format(
        row,
        c0 + 7,
        Formula::new(format!("=({qty}{r}*{fmv}{r})"))
            .set_result(num(calc.quantity * fmv_unit.unwrap_or_default()).to_string()),
        &f.whole,
    )
    .map_err(xlsx_err)?;
    ws.write_number_with_format(row, c0 + 8, num(calc.transfer_expenses), &f.money)
        .map_err(xlsx_err)?;
    Ok(())
}

fn write_shares_sheet(
    ws: &mut Worksheet,
    rows: &[&IndianCapitalGainCalculation],
    f: &Formats,
) -> Result<()> {
    ws.set_name("Sale of Shares").map_err(xlsx_err)?;
    write_headers(ws, &SHARES_HEADERS, f)?;
    for (i, calc) in rows.iter().enumerate() {
        let row = i as u32 + 1;
        let r = row + 1;
        ws.write_string(row, 0, calc.isin.as_deref().unwrap_or(""))
            .map_err(xlsx_err)?;
        ws.write_string(row, 1, &calc.asset_name)
            .map_err(xlsx_err)?;
        write_trade_columns(ws, row, 2, calc, f)?;
        let formula = format!(
            "=IF(D{r}<=DATE(2018,1,31),IF((F{r}-D{r})>365,(H{r}-K{r}-MAX(E{r},MIN(J{r},H{r}))),(H{r}-E{r}-K{r})),(H{r}-E{r}-K{r}))"
        );
        ws.write_formula_with_format(
            row,
            11,
            Formula::new(formula).set_result(num(template_net_gain(calc, true)).to_string()),
            &f.money,
        )
        .map_err(xlsx_err)?;
    }
    Ok(())
}

fn write_funds_sheet(
    ws: &mut Worksheet,
    rows: &[(&IndianCapitalGainCalculation, bool)],
    f: &Formats,
) -> Result<()> {
    ws.set_name("Sale of Mutual Funds").map_err(xlsx_err)?;
    write_headers(ws, &FUNDS_HEADERS, f)?;
    for (i, (calc, equity)) in rows.iter().enumerate() {
        let row = i as u32 + 1;
        let r = row + 1;
        ws.write_string(row, 0, if *equity { MF_EQUITY } else { MF_OTHER })
            .map_err(xlsx_err)?;
        if *equity {
            ws.write_string(row, 1, calc.isin.as_deref().unwrap_or(""))
                .map_err(xlsx_err)?;
        }
        ws.write_string(row, 2, &calc.asset_name)
            .map_err(xlsx_err)?;
        write_trade_columns(ws, row, 3, calc, f)?;
        let formula = format!(
            "=IF(E{r}<=DATE(2018,1,31),IF(A{r}=\"MF (Equity)\",IF((G{r}-E{r})>365,(I{r}-L{r}-MAX(F{r},MIN(K{r},I{r}))),(I{r}-F{r}-L{r})),(I{r}-F{r}-L{r})),I{r}-L{r}-F{r})"
        );
        ws.write_formula_with_format(
            row,
            12,
            Formula::new(formula).set_result(num(template_net_gain(calc, *equity)).to_string()),
            &f.money,
        )
        .map_err(xlsx_err)?;
    }
    let last = (rows.len() as u32).max(1);
    ws.add_data_validation(
        1,
        0,
        last.max(CLEARTAX_MAX_ROWS as u32),
        0,
        &DataValidation::new()
            .allow_list_strings(&[MF_EQUITY, MF_OTHER])
            .map_err(xlsx_err)?,
    )
    .map_err(xlsx_err)?;
    Ok(())
}

/// Builds part `part` (0-based) of the ClearTax workbook for the year, or
/// `None` when the year has no sales for ClearTax or the part is out of range.
pub fn build_cleartax_workbook(
    summary: &IndianTaxYearSummary,
    part: usize,
) -> Result<Option<Vec<u8>>> {
    let parts = cleartax_file_count(summary);
    if part >= parts {
        return Ok(None);
    }
    let rows = split_rows(summary);
    let range = |len: usize| {
        let start = (part * CLEARTAX_MAX_ROWS).min(len);
        start..((part + 1) * CLEARTAX_MAX_ROWS).min(len)
    };
    let shares = &rows.shares[range(rows.shares.len())];
    let funds = &rows.funds[range(rows.funds.len())];

    let f = Formats::new();
    let mut wb = Workbook::new();

    let ins = wb.add_worksheet();
    ins.set_name("Instructions for Sale of Shares")
        .map_err(xlsx_err)?;
    ins.set_column_width(1, 110).map_err(xlsx_err)?;
    for (row, text) in INSTRUCTIONS {
        let fmt = if row == 1 { &f.title } else { &f.wrap };
        ins.write_string_with_format(row, 1, text, fmt)
            .map_err(xlsx_err)?;
    }

    write_shares_sheet(wb.add_worksheet(), shares, &f)?;
    write_funds_sheet(wb.add_worksheet(), funds, &f)?;

    // Left empty, as in the template: F&O trades are business income, not
    // capital gains, and are out of scope here.
    let fno = wb.add_worksheet();
    fno.set_name("FnO").map_err(xlsx_err)?;
    write_headers(fno, &FNO_HEADERS, &f)?;
    let types = wb.add_worksheet();
    types.set_name("Asset Type").map_err(xlsx_err)?;
    for (row, v) in [
        "Derivative Type",
        "Options",
        "Futures",
        "Currency",
        "Commodity",
    ]
    .iter()
    .enumerate()
    {
        types.write_string(row as u32, 0, *v).map_err(xlsx_err)?;
    }

    let buf = wb.save_to_buffer().map_err(xlsx_err)?;
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lots::{HoldingPeriodCategory, IndianTaxSection};
    use rust_decimal_macros::dec;
    use std::io::Read;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn calc(
        name: &str,
        class: IndianAssetTaxClassification,
        bought: NaiveDate,
        sold: NaiveDate,
        qty: Decimal,
        cost: Decimal,
        price: Decimal,
        fmv: Option<Decimal>,
    ) -> IndianCapitalGainCalculation {
        IndianCapitalGainCalculation {
            disposal_id: format!("d-{name}"),
            lot_id: format!("l-{name}"),
            asset_id: name.to_string(),
            classification: class,
            tax_section: IndianTaxSection::Sec112a,
            acquisition_date: bought,
            disposal_date: sold,
            holding_period: HoldingPeriodCategory::LongTerm,
            actual_cost: cost,
            deemed_cost: cost,
            proceeds: qty * price,
            gross_gain: qty * price - cost,
            taxable_gain: qty * price - cost,
            applicable_tax_rate: dec!(0.125),
            estimated_tax: Decimal::ZERO,
            asset_name: name.to_string(),
            asset_symbol: None,
            isin: Some(format!("INE{name}")),
            quantity: qty,
            sale_price_per_unit: price,
            cost_per_unit: cost / qty,
            fmv_per_unit_2018: fmv,
            transfer_expenses: Decimal::ZERO,
        }
    }

    fn sheet_xml(buf: &[u8], n: usize) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(buf)).unwrap();
        let mut s = String::new();
        zip.by_name(&format!("xl/worksheets/sheet{n}.xml"))
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        s
    }

    fn workbook_xml(buf: &[u8]) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(buf)).unwrap();
        let mut s = String::new();
        zip.by_name("xl/workbook.xml")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        s
    }

    use IndianAssetTaxClassification::*;

    #[test]
    fn sales_go_to_the_right_sheet() {
        let d = date(2024, 8, 1);
        let mk = |c| calc("X", c, d, d, dec!(1), dec!(1), dec!(1), None);
        assert_eq!(
            cleartax_sheet(&mk(EquityListed)),
            Some(ClearTaxSheet::Shares)
        );
        assert_eq!(cleartax_sheet(&mk(ReitInvIt)), Some(ClearTaxSheet::Shares));
        assert_eq!(
            cleartax_sheet(&mk(EquityMf)),
            Some(ClearTaxSheet::MutualFund { equity: true })
        );
        assert_eq!(
            cleartax_sheet(&mk(DebtMf)),
            Some(ClearTaxSheet::MutualFund { equity: false })
        );
        assert_eq!(cleartax_sheet(&mk(GoldSgb)), None);
        assert_eq!(cleartax_sheet(&mk(OtherCapitalAsset)), None);
    }

    #[test]
    fn workbook_has_template_sheets_and_values() {
        let summary = IndianTaxYearSummary {
            fy_start_year: 2024,
            calculations: vec![
                // Bought before the cut-off with a 2018 price: FMV columns filled.
                calc(
                    "RELIANCE",
                    EquityListed,
                    date(2017, 6, 15),
                    date(2024, 8, 10),
                    dec!(100),
                    dec!(100000),
                    dec!(2000),
                    Some(dec!(1500)),
                ),
                calc(
                    "LIQUIDFUND",
                    DebtMf,
                    date(2023, 6, 1),
                    date(2024, 9, 1),
                    dec!(10),
                    dec!(10000),
                    dec!(1100),
                    None,
                ),
                calc(
                    "SGB",
                    GoldSgb,
                    date(2019, 1, 1),
                    date(2024, 9, 1),
                    dec!(1),
                    dec!(1),
                    dec!(2),
                    None,
                ),
            ],
            ..Default::default()
        };
        assert_eq!(cleartax_file_count(&summary), 1);
        assert_eq!(cleartax_excluded_count(&summary), 1);

        let buf = build_cleartax_workbook(&summary, 0).unwrap().unwrap();
        let wb = workbook_xml(&buf);
        for name in [
            "Instructions for Sale of Shares",
            "Sale of Shares",
            "Sale of Mutual Funds",
        ] {
            assert!(
                wb.contains(&format!("name=\"{name}\"")),
                "missing sheet {name}"
            );
        }

        let shares = sheet_xml(&buf, 2);
        // Purchase value is the actual cost; ClearTax grandfathers from the FMV.
        assert!(shares.contains("<v>100000</v>"));
        assert!(shares.contains("<v>1500</v>"));
        // Cached net gain: 2,00,000 - max(1,00,000, min(1,50,000, 2,00,000)) = 50,000.
        assert!(shares.contains("<f>IF(D2&lt;=DATE(2018,1,31)"));
        assert!(shares.contains("<v>50000</v>"));
        // Excel serial date for 15 Jun 2017.
        assert!(shares.contains("<v>42901</v>"));

        let funds = sheet_xml(&buf, 3);
        assert!(funds.contains("MF (Other than Equity)") || funds.contains("t=\"s\""));
        assert!(funds.contains("<dataValidation"));
        // Debt fund: no ISIN column value, gain = 11,000 - 10,000.
        assert!(funds.contains("<v>1000</v>"));
    }

    #[test]
    fn large_years_split_into_parts_of_5000() {
        let d = date(2024, 8, 1);
        let one = calc(
            "A",
            EquityListed,
            date(2020, 1, 1),
            d,
            dec!(1),
            dec!(1),
            dec!(2),
            None,
        );
        let summary = IndianTaxYearSummary {
            fy_start_year: 2024,
            calculations: vec![one; CLEARTAX_MAX_ROWS + 1],
            ..Default::default()
        };
        assert_eq!(cleartax_file_count(&summary), 2);
        assert!(build_cleartax_workbook(&summary, 1).unwrap().is_some());
        assert!(build_cleartax_workbook(&summary, 2).unwrap().is_none());
        assert_eq!(
            cleartax_file_name(2024, 1, 2),
            "cleartax_capital_gains_fy2024-25_part2of2.xlsx"
        );
        assert_eq!(
            cleartax_file_name(2024, 0, 1),
            "cleartax_capital_gains_fy2024-25.xlsx"
        );
    }

    #[test]
    fn empty_year_has_no_file() {
        let summary = IndianTaxYearSummary::default();
        assert_eq!(cleartax_file_count(&summary), 0);
        assert!(build_cleartax_workbook(&summary, 0).unwrap().is_none());
    }
}
