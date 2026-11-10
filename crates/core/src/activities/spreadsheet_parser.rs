//! Excel (.xlsx / .xls / .ods) broker reports.
//!
//! The first sheet is read as a table: the header is the first row with at
//! least three text cells, and data runs until the first empty row. Known
//! reports are reshaped for their import template:
//!
//! - Upstox tradebook (`Date, Company, Amount, Exchange, Segment, Scrip Code,
//!   …, Side, Quantity, Price`). It has no ISIN, so the BSE scrip code becomes
//!   the symbol (`543983.BO`). Only cash-market equity rows are kept.
//!
//! Any other sheet is returned as is, for the user to map.

use std::io::Cursor;

use calamine::{open_workbook_auto_from_rs, Data, Reader};

use super::csv_parser::{ParseConfig, ParsedCsvResult};
use super::pdf_parser::{ParsedPdfResult, StatementType};
use crate::errors::{Error, ValidationError};
use crate::Result;

/// Template for Upstox tradebooks, seeded by migration.
pub const UPSTOX_TEMPLATE_ID: &str = "system_upstox";

/// Output columns for an Upstox tradebook.
pub const UPSTOX_HEADERS: [&str; 10] = [
    "Date",
    "Side",
    "Company",
    "Symbol",
    "Scrip Code",
    "Exchange",
    "Quantity",
    "Price",
    "Amount",
    "Trade Num",
];

/// True for the zip container of .xlsx / .ods, or the OLE container of .xls.
pub fn looks_like_spreadsheet(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0])
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(d) if d.time() == chrono::NaiveTime::MIN => d.format("%Y-%m-%d").to_string(),
            Some(d) => d.format("%Y-%m-%d %H:%M:%S").to_string(),
            None => dt.as_f64().to_string(),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.trim().to_string(),
    }
}

/// Header and data rows of the first table in a sheet.
fn first_table(rows: Vec<Vec<String>>) -> (Vec<String>, Vec<Vec<String>>) {
    let header_idx = rows
        .iter()
        .position(|r| r.iter().filter(|c| !c.is_empty()).count() >= 3);
    let Some(header_idx) = header_idx else {
        return (Vec::new(), Vec::new());
    };
    let mut headers = rows[header_idx].clone();
    while headers.last().is_some_and(|h| h.is_empty()) {
        headers.pop();
    }
    let width = headers.len();
    let data = rows[header_idx + 1..]
        .iter()
        .take_while(|r| r.iter().any(|c| !c.is_empty()))
        .map(|r| {
            let mut row: Vec<String> = r.iter().take(width).cloned().collect();
            row.resize(width, String::new());
            row
        })
        .collect();
    (headers, data)
}

fn is_upstox_tradebook(headers: &[String]) -> bool {
    let has = |name: &str| headers.iter().any(|h| h.eq_ignore_ascii_case(name));
    has("Scrip Code") && has("Side") && has("Trade Num") && has("Company")
}

fn reshape_upstox(headers: &[String], rows: &[Vec<String>]) -> Vec<Vec<String>> {
    let col = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let get = |row: &Vec<String>, name: &str| {
        col(name)
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or_default()
    };
    rows.iter()
        .filter(|row| {
            // Cash-market equity only; F&O, currency and commodity are not
            // supported yet.
            let segment = get(row, "Segment").to_uppercase();
            let instrument = get(row, "Instrument Type").to_uppercase();
            (segment.is_empty() || segment == "EQ")
                && (instrument.is_empty() || instrument == "EQUITY")
        })
        .map(|row| {
            let scrip = get(row, "Scrip Code");
            let symbol = if !scrip.is_empty() && scrip.chars().all(|c| c.is_ascii_digit()) {
                format!("{scrip}.BO")
            } else {
                scrip.clone()
            };
            vec![
                get(row, "Date"),
                get(row, "Side"),
                get(row, "Company"),
                symbol,
                scrip,
                get(row, "Exchange"),
                get(row, "Quantity"),
                get(row, "Price"),
                get(row, "Amount"),
                get(row, "Trade Num"),
            ]
        })
        .collect()
}

/// Parses the first sheet of a spreadsheet into import rows.
pub fn parse_spreadsheet(content: &[u8]) -> Result<ParsedPdfResult> {
    let invalid = |msg: String| Error::Validation(ValidationError::InvalidInput(msg));
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(content.to_vec()))
        .map_err(|e| invalid(format!("Failed to open spreadsheet: {e}")))?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| invalid("The spreadsheet has no sheets.".to_string()))?
        .map_err(|e| invalid(format!("Failed to read spreadsheet: {e}")))?;

    let cells: Vec<Vec<String>> = range
        .rows()
        .map(|r| r.iter().map(cell_text).collect())
        .collect();
    let (headers, rows) = first_table(cells);
    if headers.is_empty() {
        return Err(invalid("No table found in the spreadsheet.".to_string()));
    }

    let (headers, rows, statement_type) = if is_upstox_tradebook(&headers) {
        (
            UPSTOX_HEADERS.iter().map(|h| h.to_string()).collect(),
            reshape_upstox(&headers, &rows),
            StatementType::UpstoxTradebook,
        )
    } else {
        (headers, rows, StatementType::Unknown)
    };

    let row_count = rows.len();
    Ok(ParsedPdfResult {
        parsed: ParsedCsvResult {
            headers,
            rows,
            detected_config: ParseConfig {
                delimiter: Some(",".to_string()),
                date_format: Some("auto".to_string()),
                decimal_separator: Some(".".to_string()),
                thousands_separator: Some(",".to_string()),
                has_header_row: Some(true),
                ..Default::default()
            },
            errors: Vec::new(),
            row_count,
        },
        recommended_template_id: statement_type.default_template_id().map(str::to_string),
        statement_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn finds_the_table_under_a_report_header() {
        let rows = vec![
            s(&["EXAMPLE BROKER LIMITED", "", "", ""]),
            s(&["UCC", "AB0001", "", ""]),
            s(&["", "", "", ""]),
            s(&["Date", "Company", "Side", "Quantity", ""]),
            s(&["2026-01-16", "EXAMPLE LIMITED", "Sell", "3"]),
            s(&["2026-01-17", "EXAMPLE LIMITED", "Buy", "2"]),
            s(&["", "", "", ""]),
            s(&["Footer note", "", "", ""]),
        ];
        let (headers, data) = first_table(rows);
        assert_eq!(headers, s(&["Date", "Company", "Side", "Quantity"]));
        assert_eq!(data.len(), 2);
        assert_eq!(data[1], s(&["2026-01-17", "EXAMPLE LIMITED", "Buy", "2"]));
    }

    #[test]
    fn reshapes_upstox_rows_and_keeps_equity_only() {
        let headers = s(&[
            "Date",
            "Company",
            "Amount",
            "Exchange",
            "Segment",
            "Scrip Code",
            "Instrument Type",
            "Strike Price",
            "Expiry",
            "Trade Num",
            "Trade Time",
            "Side",
            "Quantity",
            "Price",
        ]);
        assert!(is_upstox_tradebook(&headers));
        let rows = vec![
            s(&[
                "2026-01-16",
                "EXAMPLE LIMITED",
                "1149.9",
                "NSE",
                "EQ",
                "500001",
                "Equity",
                "0",
                "",
                "200000001",
                "09:23:16",
                "Sell",
                "3",
                "383.3",
            ]),
            s(&[
                "2026-01-16",
                "EXAMPLE FUT",
                "100",
                "NSE",
                "FO",
                "EXFUT",
                "Futures",
                "0",
                "2026-01-29",
                "200000002",
                "09:30:00",
                "Buy",
                "50",
                "2",
            ]),
        ];
        let out = reshape_upstox(&headers, &rows);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0],
            s(&[
                "2026-01-16",
                "Sell",
                "EXAMPLE LIMITED",
                "500001.BO",
                "500001",
                "NSE",
                "3",
                "383.3",
                "1149.9",
                "200000001",
            ])
        );
    }

    #[test]
    fn cell_text_formats_numbers_and_dates() {
        assert_eq!(cell_text(&Data::Float(3.0)), "3");
        assert_eq!(cell_text(&Data::Float(383.25)), "383.25");
        assert_eq!(cell_text(&Data::String("  Sell ".into())), "Sell");
        assert_eq!(cell_text(&Data::Empty), "");
    }

    #[test]
    fn detects_spreadsheet_containers() {
        assert!(looks_like_spreadsheet(b"PK\x03\x04rest"));
        assert!(!looks_like_spreadsheet(b"%PDF-1.7"));
    }
}
