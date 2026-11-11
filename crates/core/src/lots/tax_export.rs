//! Schedule CG / ITR export formatters for Indian capital gains.

use rust_decimal::Decimal;
use serde::Serialize;

use super::{HoldingPeriodCategory, IndianTaxSection, IndianTaxYearSummary};

/// A single row in the Schedule CG CSV export.
/// Column order matches the ITR-2/ITR-3 Schedule CG format.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCgRow {
    pub isin: String,
    pub asset_name: String,
    pub symbol: String,
    pub section: String,
    pub holding_period: String,
    pub acquisition_date: String,
    pub disposal_date: String,
    pub quantity: Decimal,
    pub sale_price_per_unit: Decimal,
    pub full_sale_consideration: Decimal,
    pub cost_of_acquisition: Decimal,
    pub fmv_per_unit_jan_31_2018: Option<Decimal>,
    pub deemed_cost_of_acquisition: Decimal,
    pub transfer_expenses: Decimal,
    pub total_deductions: Decimal,
    pub capital_gain: Decimal,
    pub applicable_tax_rate_pct: Decimal,
    pub estimated_tax: Decimal,
}

/// First tax year under the Income-tax Act 2025 (2026-27), which renumbers
/// the sections: 111A is 196, 112 is 197 and 112A is 198.
pub const INCOME_TAX_ACT_2025_FIRST_YEAR: i32 = 2026;

/// ITR label for a row's tax section in financial year `fy_start_year`.
fn section_label(section: IndianTaxSection, fy_start_year: i32) -> &'static str {
    let act_2025 = fy_start_year >= INCOME_TAX_ACT_2025_FIRST_YEAR;
    match section {
        IndianTaxSection::Sec111a if act_2025 => "Section 196 (formerly 111A)",
        IndianTaxSection::Sec112a if act_2025 => "Section 198 (formerly 112A)",
        IndianTaxSection::Sec112 if act_2025 => "Section 197 (formerly 112)",
        IndianTaxSection::Sec111a => "Section 111A",
        IndianTaxSection::Sec112a => "Section 112A",
        IndianTaxSection::Sec112 => "Section 112",
        IndianTaxSection::Sec50aa => "Section 50AA (Slab Rate)",
        IndianTaxSection::SlabStcg => "Slab Rate",
        IndianTaxSection::Sec10_38 => "Section 10(38) (Exempt)",
    }
}

fn holding_period_label(hp: HoldingPeriodCategory) -> &'static str {
    match hp {
        HoldingPeriodCategory::ShortTerm => "Short Term",
        HoldingPeriodCategory::LongTerm => "Long Term",
    }
}

/// Converts an `IndianTaxYearSummary` into Schedule CG export rows.
pub fn to_schedule_cg_rows(summary: &IndianTaxYearSummary) -> Vec<ScheduleCgRow> {
    summary
        .calculations
        .iter()
        .map(|calc| {
            let total_deductions = calc.deemed_cost + calc.transfer_expenses;
            ScheduleCgRow {
                isin: calc.isin.clone().unwrap_or_default(),
                asset_name: calc.asset_name.clone(),
                symbol: calc.asset_symbol.clone().unwrap_or_default(),
                section: section_label(calc.tax_section, summary.fy_start_year).to_string(),
                holding_period: holding_period_label(calc.holding_period).to_string(),
                acquisition_date: calc.acquisition_date.format("%d/%m/%Y").to_string(),
                disposal_date: calc.disposal_date.format("%d/%m/%Y").to_string(),
                quantity: calc.quantity,
                sale_price_per_unit: calc.sale_price_per_unit,
                full_sale_consideration: calc.proceeds,
                cost_of_acquisition: calc.actual_cost,
                fmv_per_unit_jan_31_2018: calc.fmv_per_unit_2018,
                deemed_cost_of_acquisition: calc.deemed_cost,
                transfer_expenses: calc.transfer_expenses,
                total_deductions,
                capital_gain: calc.gross_gain,
                applicable_tax_rate_pct: calc.applicable_tax_rate * Decimal::ONE_HUNDRED,
                estimated_tax: calc.estimated_tax,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lots::{IndianAssetTaxClassification, IndianCapitalGainCalculation};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    #[test]
    fn section_labels_correct() {
        let label = |s| section_label(s, 2025);
        assert_eq!(label(IndianTaxSection::Sec111a), "Section 111A");
        assert_eq!(label(IndianTaxSection::Sec112a), "Section 112A");
        assert_eq!(label(IndianTaxSection::Sec50aa), "Section 50AA (Slab Rate)");
        assert_eq!(label(IndianTaxSection::Sec112), "Section 112");
    }

    #[test]
    fn section_labels_follow_the_2025_act_from_2026_27() {
        let label = |s| section_label(s, 2026);
        assert_eq!(
            label(IndianTaxSection::Sec111a),
            "Section 196 (formerly 111A)"
        );
        assert_eq!(
            label(IndianTaxSection::Sec112),
            "Section 197 (formerly 112)"
        );
        assert_eq!(
            label(IndianTaxSection::Sec112a),
            "Section 198 (formerly 112A)"
        );
    }

    #[test]
    fn to_schedule_cg_rows_produces_correct_output() {
        let summary = IndianTaxYearSummary {
            calculations: vec![IndianCapitalGainCalculation {
                disposal_id: "d1".to_string(),
                lot_id: "l1".to_string(),
                asset_id: "a1".to_string(),
                classification: IndianAssetTaxClassification::EquityListed,
                tax_section: IndianTaxSection::Sec112a,
                acquisition_date: NaiveDate::from_ymd_opt(2017, 6, 15).unwrap(),
                disposal_date: NaiveDate::from_ymd_opt(2024, 8, 10).unwrap(),
                holding_period: HoldingPeriodCategory::LongTerm,
                actual_cost: dec!(100000),
                deemed_cost: dec!(150000),
                proceeds: dec!(200000),
                gross_gain: dec!(50000),
                taxable_gain: dec!(50000),
                applicable_tax_rate: dec!(0.125),
                estimated_tax: dec!(6250),
                asset_name: "Reliance Industries".to_string(),
                asset_symbol: Some("RELIANCE".to_string()),
                isin: Some("INE002A01018".to_string()),
                quantity: dec!(100),
                sale_price_per_unit: dec!(2000),
                cost_per_unit: dec!(1000),
                fmv_per_unit_2018: Some(dec!(1500)),
                transfer_expenses: dec!(0),
            }],
            ..Default::default()
        };

        let rows = to_schedule_cg_rows(&summary);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.section, "Section 112A");
        assert_eq!(row.isin, "INE002A01018");
        assert_eq!(row.acquisition_date, "15/06/2017");
        assert_eq!(row.disposal_date, "10/08/2024");
        assert_eq!(row.quantity, dec!(100));
        assert_eq!(row.full_sale_consideration, dec!(200000));
        assert_eq!(row.deemed_cost_of_acquisition, dec!(150000));
        assert_eq!(row.capital_gain, dec!(50000));
        assert_eq!(row.applicable_tax_rate_pct, dec!(12.5));
    }
}
