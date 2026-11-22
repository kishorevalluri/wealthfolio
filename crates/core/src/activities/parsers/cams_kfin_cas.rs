//! CAMS / KFintech mutual fund Consolidated Account Statement (CAS) parser.
//!
//! The detailed CAS lists each folio, then each scheme in it with an ISIN,
//! then its transactions:
//!
//! ```text
//! Folio No: 12345678 / 0   PAN: ...   KYC: OK  PAN: OK
//! B78G-Example Large & Midcap Fund - Direct Plan - Growth (Non-Demat) - ISIN: INF000X01AA1(Advisor: DIRECT) Registrar : KFINTECH
//! Opening Unit Balance: 0.000
//! 02-Nov-2020 Purchase - via Internet 4,999.75 77.463 64.5440 77.463
//! 02-Nov-2020 *** Stamp Duty *** 0.25
//! 18-Jul-2025 Redemption (100,001.00) (587.464) 170.2249 0.000
//! 18-Jul-2025 *** STT Paid *** 1.00
//! Closing Unit Balance: 0.000
//! ```
//!
//! A transaction is a date, a description and four numbers: amount, units,
//! NAV and the unit balance after it; outflows are in brackets or negative.
//! A row can break over several lines, so the parser reads the statement as
//! one stream of words, each row running from its date to the next date.
//!
//! Each purchase or redemption becomes one row. A purchase's amount is net of
//! stamp duty and a redemption's is net of STT, so the price is the value per
//! unit before them. Stamp duty is a purchase charge (cost of acquisition),
//! so it is the row's fee; STT on a redemption is its tax. A reversed or
//! rejected transaction is left out together with the row it reverses.

use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;
use rust_decimal::Decimal;

/// Columns the parser writes, in order (template `system_cams_cas`).
pub const CAMS_HEADERS: [&str; 11] = [
    "Date",
    "Transaction Description",
    "Scheme Name",
    "ISIN",
    "Units",
    "NAV",
    "Amount",
    "Folio No",
    "Fee",
    "Tax",
    "Comment",
];

/// `Transaction Description` values the parser writes.
pub const PURCHASE: &str = "PURCHASE";
pub const REDEMPTION: &str = "REDEMPTION";
pub const SWITCH_IN: &str = "SWITCH IN";
pub const SWITCH_OUT: &str = "SWITCH OUT";
pub const DIVIDEND: &str = "DIVIDEND";
pub const REINVESTMENT: &str = "REINVESTMENT";
/// A scheme merger's two legs. They carry the units' cost and dates to the
/// new scheme (no capital gain, Section 47(xviii)); the import wizard pairs
/// them into a conversion, as it pairs a depository's corporate actions.
pub const MERGER_OUT: &str = "CORPORATE ACTION DEBIT";
pub const MERGER_IN: &str = "CORPORATE ACTION CREDIT";

const DATE: &str = r"\d{2}-[A-Za-z]{3}-\d{4}";
const SIGNED: &str = r"\(?-?[\d,]+\.\d+\)?";

static DATE_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\b{DATE}\b")).expect("hard-coded regex is valid"));

static FOLIO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)Folio\s*No\s*:\s*([0-9A-Za-z]+(?:\s*/\s*[0-9A-Za-z]+)?)")
        .expect("hard-coded regex is valid")
});

/// The end of a scheme's heading: "(Non-Demat) (Advisor:DIRECT) - ISIN:INF...".
/// The heading can run over lines and the name can hold brackets.
static SCHEME_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\(\s*(?:[Nn]on[- ]?)?[Dd]emat\s*\)\s*(?:Registrar\s*:\s*\w+\s*)?(?:\(\s*Advisor\s*:[^)]*\)\s*)?-?\s*(?:Registrar\s*:\s*\w+\s*)?ISIN\s*:\s*(IN[FE][A-Z0-9]{9})",
    )
    .expect("hard-coded regex is valid")
});

/// Text a scheme's heading follows: the last of these before it bounds where
/// its name can start.
static SCHEME_BOUNDARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)Closing Unit\s+Balance|Nominee\s*\d\s*:|Balance\s*\(INR\)|Mutual Fund\b|KYC\s*:\s*\w+\s*PAN\s*:\s*\w+",
    )
    .expect("hard-coded regex is valid")
});

/// The scheme code that starts a heading: "B78G-", "128STDGG-", "127 FMGDG-".
static SCHEME_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)(?:\d{3}\s)?[A-Z0-9]{2,12}\s?-\s?").expect("hard-coded regex is valid")
});

/// "Registrar : CAMS", printed beside a scheme's heading.
static REGISTRAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Registrar\s*:\s*\w+").expect("hard-coded regex is valid"));

static TRANSACTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^({DATE})\s+(\*?[^*]{{1,160}}?)\s+({SIGNED})\s+({SIGNED})\s+([\d,]+\.\d+)\s+({SIGNED})"
    ))
    .expect("hard-coded regex is valid")
});

/// "18-Jul-2025 *** STT Paid *** 1.00".
static CHARGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^({DATE})\s+\*\*\*\s*([^*]{{1,60}}?)\s*\*\*\*\s+({SIGNED})"
    ))
    .expect("hard-coded regex is valid")
});

/// "10-Mar-2023 IDCW Paid @ Rs.1.00 per unit 1,234.00": a payout with no units.
static PAYOUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^({DATE})\s+((?:IDCW|Dividend)[^*]{{0,80}}?(?:Paid|Payout)[^*]{{0,40}}?)\s+([\d,]+\.\d{{2}})(?:\s|$)"
    ))
    .expect("hard-coded regex is valid")
});

#[derive(Debug, Clone)]
struct Row {
    date: String,
    kind: &'static str,
    scheme: String,
    isin: String,
    units: Decimal,
    nav: Decimal,
    amount: Decimal,
    folio: String,
    fee: Decimal,
    tax: Decimal,
    comment: String,
}

impl Row {
    fn is_inflow(&self) -> bool {
        matches!(self.kind, PURCHASE | SWITCH_IN | REINVESTMENT | MERGER_IN)
    }

    fn cells(&self) -> Vec<String> {
        // A purchase's amount is what bought units, after stamp duty; a
        // redemption's is what was paid out, after STT. The price is the
        // value per unit before those charges: units × price is the amount
        // for a purchase, and the amount plus STT for a redemption.
        let gross = if self.is_inflow() {
            self.amount
        } else {
            self.amount + self.tax + self.fee
        };
        let price = if self.units.is_zero() {
            self.nav
        } else {
            (gross / self.units).round_dp(8)
        };
        let units = if self.kind == DIVIDEND {
            String::new()
        } else {
            self.units.normalize().to_string()
        };
        vec![
            self.date.clone(),
            self.kind.to_string(),
            self.scheme.clone(),
            self.isin.clone(),
            units,
            if self.kind == DIVIDEND {
                String::new()
            } else {
                price.normalize().to_string()
            },
            self.amount.normalize().to_string(),
            self.folio.clone(),
            self.fee.normalize().to_string(),
            self.tax.normalize().to_string(),
            self.comment.clone(),
        ]
    }
}

/// A number as the statement prints it; brackets or a minus are negative.
fn number(raw: &str) -> Option<Decimal> {
    let negative = raw.contains('(') || raw.starts_with('-');
    let digits: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value = Decimal::from_str(&digits).ok()?;
    Some(if negative { -value } else { value })
}

fn clean_scheme(name: &str) -> String {
    let name = REGISTRAR.replace_all(name, " ");
    let mut name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    for tail in ["(Non-Demat)", "(Demat)", "(non-demat)", "(demat)"] {
        if let Some(stripped) = name.strip_suffix(tail) {
            name = stripped.trim().to_string();
        }
    }
    name.trim_end_matches(['-', ' ']).trim().to_string()
}

enum Event {
    Folio(String),
    Scheme(String, String),
    Transaction {
        date: String,
        desc: String,
        amount: Decimal,
        units: Decimal,
        nav: Decimal,
        balance: String,
    },
    Charge {
        date: String,
        desc: String,
        amount: Decimal,
    },
    Payout {
        date: String,
        desc: String,
        amount: Decimal,
    },
}

/// Parse a CAMS / KFintech CAS into rows (see [`CAMS_HEADERS`]).
pub fn parse_cams_kfin_cas(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = CAMS_HEADERS.iter().map(|h| h.to_string()).collect();
    let stream = text.split_whitespace().collect::<Vec<_>>().join(" ");

    let mut events: Vec<(usize, Event)> = Vec::new();
    for caps in FOLIO.captures_iter(&stream) {
        let m = caps.get(0).expect("match");
        let folio: String = caps[1].chars().filter(|c| !c.is_whitespace()).collect();
        events.push((m.start(), Event::Folio(folio)));
    }
    let boundaries: Vec<usize> = SCHEME_BOUNDARY
        .find_iter(&stream)
        .map(|m| m.end())
        .collect();
    let mut previous_tail = 0usize;
    for caps in SCHEME_TAIL.captures_iter(&stream) {
        let tail = caps.get(0).expect("match");
        // The heading starts at the first scheme code after the last boundary.
        let from = boundaries
            .iter()
            .copied()
            .filter(|&b| b <= tail.start())
            .max()
            .unwrap_or(0)
            .max(previous_tail);
        previous_tail = tail.end();
        let window = &stream[from..tail.start()];
        let (start, name) = match SCHEME_CODE.find(window) {
            Some(code) => (from + code.start(), &window[code.end()..]),
            None => (from, window),
        };
        events.push((
            start,
            Event::Scheme(clean_scheme(name), caps[1].to_uppercase()),
        ));
    }
    // Each transaction starts with its date and ends before the next date,
    // so a row is read within its own stretch of text.
    let starts: Vec<usize> = DATE_START.find_iter(&stream).map(|m| m.start()).collect();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(stream.len());
        let segment = &stream[start..end];
        if let Some(caps) = TRANSACTION.captures(segment) {
            if let (Some(amount), Some(units), Some(nav)) =
                (number(&caps[3]), number(&caps[4]), number(&caps[5]))
            {
                events.push((
                    start,
                    Event::Transaction {
                        date: caps[1].to_string(),
                        desc: caps[2].trim().to_string(),
                        amount,
                        units,
                        nav,
                        balance: caps[6].to_string(),
                    },
                ));
            }
        } else if let Some(caps) = CHARGE.captures(segment) {
            if let Some(amount) = number(&caps[3]) {
                events.push((
                    start,
                    Event::Charge {
                        date: caps[1].to_string(),
                        desc: caps[2].trim().to_string(),
                        amount: amount.abs(),
                    },
                ));
            }
        } else if let Some(caps) = PAYOUT.captures(segment) {
            if let Some(amount) = number(&caps[3]) {
                events.push((
                    start,
                    Event::Payout {
                        date: caps[1].to_string(),
                        desc: caps[2].trim().to_string(),
                        amount,
                    },
                ));
            }
        }
    }
    events.sort_by_key(|(start, _)| *start);

    let mut rows: Vec<Row> = Vec::new();
    let mut folio = String::new();
    let mut scheme = String::new();
    let mut isin = String::new();
    for (_, event) in events {
        match event {
            Event::Folio(f) => folio = f,
            Event::Scheme(name, code) => {
                scheme = name;
                isin = code;
            }
            Event::Transaction {
                date,
                desc,
                amount,
                units,
                nav,
                balance,
            } => {
                if isin.is_empty() {
                    continue;
                }
                let lower = desc.to_lowercase();
                if ["reversal", "reversed", "rejection", "rejected", "invalid"]
                    .iter()
                    .any(|w| lower.contains(w))
                {
                    // Drop the row this one reverses: same scheme and units.
                    if let Some(index) = rows.iter().rposition(|r| {
                        r.isin == isin
                            && r.folio == folio
                            && r.units == units.abs()
                            && r.is_inflow() == units.is_sign_negative()
                            && r.kind != DIVIDEND
                    }) {
                        rows.remove(index);
                    }
                    continue;
                }
                if units.is_zero() {
                    continue;
                }
                let base = Row {
                    date: date.clone(),
                    kind: PURCHASE,
                    scheme: scheme.clone(),
                    isin: isin.clone(),
                    units: units.abs(),
                    nav,
                    amount: amount.abs(),
                    folio: folio.clone(),
                    fee: Decimal::ZERO,
                    tax: Decimal::ZERO,
                    // The unit balance after the row tells apart identical
                    // purchases on one day (the import skips duplicates).
                    comment: format!("CAS: {desc} (balance {balance})"),
                };
                let reinvest = lower.contains("reinvest")
                    && (lower.contains("idcw")
                        || lower.contains("dividend")
                        || lower.contains("div."));
                if reinvest {
                    rows.push(Row {
                        kind: DIVIDEND,
                        ..base.clone()
                    });
                    rows.push(Row {
                        kind: REINVESTMENT,
                        ..base
                    });
                } else if units.is_sign_positive() {
                    let kind = if lower.contains("merger") {
                        MERGER_IN
                    } else if lower.contains("switch") {
                        SWITCH_IN
                    } else {
                        PURCHASE
                    };
                    rows.push(Row { kind, ..base });
                } else {
                    let kind = if lower.contains("merger") {
                        MERGER_OUT
                    } else if lower.contains("switch") {
                        SWITCH_OUT
                    } else {
                        REDEMPTION
                    };
                    rows.push(Row { kind, ..base });
                }
            }
            Event::Charge { date, desc, amount } => {
                let lower = desc.to_lowercase();
                let stamp = lower.contains("stamp");
                let stt = lower.contains("stt") || lower.contains("securities transaction");
                if !stamp && !stt {
                    continue;
                }
                if let Some(row) = rows.iter_mut().rev().find(|r| {
                    r.date == date
                        && r.isin == isin
                        && r.folio == folio
                        && r.kind != DIVIDEND
                        && r.is_inflow() == stamp
                }) {
                    if stamp {
                        row.fee += amount;
                    } else {
                        row.tax += amount;
                    }
                }
            }
            Event::Payout { date, desc, amount } => {
                if isin.is_empty() {
                    continue;
                }
                rows.push(Row {
                    date,
                    kind: DIVIDEND,
                    scheme: scheme.clone(),
                    isin: isin.clone(),
                    units: Decimal::ZERO,
                    nav: Decimal::ZERO,
                    amount,
                    folio: folio.clone(),
                    fee: Decimal::ZERO,
                    tax: Decimal::ZERO,
                    comment: format!("CAS: {desc}"),
                });
            }
        }
    }

    (headers, rows.iter().map(Row::cells).collect())
}

/// Whether the text is a CAMS / KFintech CAS: only these list each scheme's
/// "Opening Unit Balance". They name the depositories too (demat folios), so
/// this is checked before the depository statements.
pub fn is_cams_kfin_cas(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("opening unit balance")
        && (lower.contains("cams") || lower.contains("kfin") || lower.contains("karvy"))
        && !lower.contains("bo id")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn by_header(rows: &[Vec<String>]) -> Vec<HashMap<&'static str, String>> {
        rows.iter()
            .map(|r| {
                CAMS_HEADERS
                    .iter()
                    .copied()
                    .zip(r.iter().cloned())
                    .collect()
            })
            .collect()
    }

    // Synthetic statement: made-up folios, ISINs and amounts.
    const SAMPLE: &str = r#"
CONSOLIDATED ACCOUNT STATEMENT
CAMS + KFintech
Example Mutual Fund
Folio No: 1234567 / 89 PAN: XXXXX0000X KYC: OK PAN: OK
TEST INVESTOR
B78G-Example Large & Midcap Fund - Direct Plan - Growth (Non-Demat) - ISIN: INF000X01AA1(Advisor: DIRECT) Registrar : KFINTECH
Opening Unit Balance: 0.000
01-Jun-2020 Purchase - via Internet 2,429.00 46.919 51.7700 46.919
02-Nov-2020 SIP Purchase - Instalment 5/120
4,999.75 77.463 64.5440 124.382
02-Nov-2020 *** Stamp Duty *** 0.25
03-Dec-2020 Purchase 1,000.00 15.000 66.6667 139.382
04-Dec-2020 Purchase Reversal (1,000.00) (15.000) 66.6667 124.382
18-Jul-2025 Redemption (21,172.92) (124.382) 170.2249 0.000
18-Jul-2025 *** STT Paid *** 0.21
Closing Unit Balance: 0.000 NAV on 31-Oct-2026: INR 180.00 Total Cost Value: 0.00 Market Value on 31-Oct-2026: INR 0.00

Folio No: 7654321 PAN: XXXXX0000X KYC: OK PAN: OK
P123-Example Debt Fund - Regular Plan - IDCW (Non-Demat) - ISIN: INF000X01BB2(Advisor: ARN-0000) Registrar : CAMS
Opening Unit Balance: 100.000
10-Mar-2023 IDCW Paid @ Rs.1.00 per unit 100.00
11-Apr-2023 IDCW Reinvestment @ Rs.0.50 per unit 50.00 4.545 11.0000 104.545
12-May-2023 Switch-Out - To Example Equity Fund (500.00) (45.454) 11.0000 59.091
13-Jun-2023 *Redemption - N000000 (110.00) (10.000) 11.0000 49.091
14-Jul-2023 Switch-Out (Merger) - To Example Hybrid Fund (539.00) (49.091) 10.9796 0.000
Closing Unit Balance: 0.000
"#;

    #[test]
    fn reads_purchases_redemptions_and_their_charges() {
        let (headers, rows) = parse_cams_kfin_cas(SAMPLE);
        assert_eq!(headers, CAMS_HEADERS.map(String::from).to_vec());
        let rows = by_header(&rows);
        let kinds: Vec<&str> = rows
            .iter()
            .map(|r| r["Transaction Description"].as_str())
            .collect();
        assert_eq!(
            kinds,
            vec![
                PURCHASE,
                PURCHASE,
                REDEMPTION,
                DIVIDEND,
                DIVIDEND,
                REINVESTMENT,
                SWITCH_OUT,
                REDEMPTION,
                MERGER_OUT
            ]
        );

        let first = &rows[0];
        assert_eq!(
            first["Scheme Name"],
            "Example Large & Midcap Fund - Direct Plan - Growth"
        );
        assert_eq!(first["ISIN"], "INF000X01AA1");
        assert_eq!(first["Folio No"], "1234567/89");
        assert_eq!(first["Date"], "01-Jun-2020");
        assert_eq!(first["Units"], "46.919");

        // A row broken over two lines; stamp duty is its fee.
        let sip = &rows[1];
        assert_eq!(sip["Date"], "02-Nov-2020");
        assert_eq!(sip["Units"], "77.463");
        assert_eq!(sip["Amount"], "4999.75");
        assert_eq!(sip["Fee"], "0.25");
        let price = Decimal::from_str(&sip["NAV"]).unwrap();
        assert_eq!(
            (price * Decimal::from_str("77.463").unwrap()).round_dp(2),
            Decimal::from_str("4999.75").unwrap()
        );

        // The reversed purchase is gone; the redemption carries its STT.
        let sale = &rows[2];
        // Paid out 21,172.92 after 0.21 STT: units × price is 21,173.13.
        let price = Decimal::from_str(&sale["NAV"]).unwrap();
        assert_eq!(
            (price * Decimal::from_str("124.382").unwrap()).round_dp(2),
            Decimal::from_str("21173.13").unwrap()
        );
        assert_eq!(sale["Units"], "124.382");
        assert_eq!(sale["Amount"], "21172.92");
        assert_eq!(sale["Tax"], "0.21");
        assert_eq!(sale["Fee"], "0");

        let payout = &rows[3];
        assert_eq!(payout["Folio No"], "7654321");
        assert_eq!(payout["ISIN"], "INF000X01BB2");
        assert_eq!(payout["Amount"], "100");
        assert_eq!(payout["Units"], "");

        assert_eq!(rows[4]["Amount"], "50");
        assert_eq!(rows[5]["Units"], "4.545");
        assert_eq!(rows[6]["Units"], "45.454");
    }

    #[test]
    fn recognises_the_statement() {
        assert!(is_cams_kfin_cas(SAMPLE));
        assert!(!is_cams_kfin_cas(
            "Central Depository Services BO ID 1234 Opening Unit Balance CAMS"
        ));
    }
}
