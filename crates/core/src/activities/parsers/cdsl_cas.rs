//! CDSL e-CAS (Central Depository Services Limited) parser.
//!
//! Handles CDSL Consolidated Account Statements (CAS) containing both
//! Demat Equity & Mutual Fund Holdings and Monthly Transactions.
//!
//! Features:
//! - Filters out Hindi / Devanagari bilingual headers and portfolio total summary rows.
//! - Cleans verbose corporate action noise from security names.
//! - Extracts valid ISIN holding balances and monthly buy/sell transactions.

use std::str::FromStr;

use regex::Regex;
use rust_decimal::Decimal;

/// Checks if a string contains any Devanagari characters (Hindi text).
pub fn is_hindi_text(s: &str) -> bool {
    s.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
}

/// Checks if a line is a portfolio-level summary row that should not be parsed as a holding or transaction.
pub fn is_summary_or_noise_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    is_hindi_text(line)
        || lower.contains("portfolio valuation")
        || lower.contains("portfolio value")
        || lower.contains("total portfolio")
        || lower.contains("consolidated account statement")
        || lower.contains("central depository services")
        || lower.contains("page ")
        || lower.contains("statement period")
        || lower.contains("report date")
        || lower.contains("market value as on")
}

static EQUITY_SUFFIX: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)\s*-?\s*(NEW\s*)?EQUITY\s*SHARES").expect("hard-coded regex is valid")
});

/// Cleans CDSL security descriptions.
///
/// Strips corporate action suffixes like:
/// - `# NEW EQUITY SHARES WITH FV RS 2/- AFTER CONSOLIDATION`
/// - `#NEW EQUITY SHARES WITH FACE VALUE RE.1/- AFTER SUB-DIVISION`
/// - `-EQUITY SHARES`
/// - `# EQUITY SHARES`
pub fn clean_security_description(desc: &str) -> String {
    let mut cleaned = desc.trim();

    // If description contains '#', check if it's an ETF scheme name or a corporate action suffix
    if let Some(idx) = cleaned.find('#') {
        let after_hash = cleaned[idx + 1..].trim();
        let after_upper = after_hash.to_uppercase();
        if after_upper.contains("SGB") {
            // Sovereign gold bonds: "GOVT OF INDIA#2.5% SGB 2020-21 SERIES V
            // 2028-LOAN CODE 29278 - RED DT 11.08.2028". The bond is named
            // after the '#'; the issuer alone would name every SGB the same.
            cleaned = after_hash;
            if let Some(end) = cleaned.to_uppercase().find("-LOAN") {
                cleaned = cleaned[..end].trim();
            }
        } else if after_upper.contains("ETF")
            || after_upper.contains("GOLDBEES")
            || after_upper.contains("GOLD BEES")
            || after_upper.contains("LIQUID BEES")
            || after_upper.contains("LIQUIDBEES")
            || after_upper.contains("NIFTY BEES")
            || after_upper.contains("NIFTYBEES")
            || after_upper.contains("NASDAQ 100")
            || after_upper.contains("NASDAQ100")
            || after_upper.contains("INDEX FUND")
        {
            // For ETFs (e.g. "NIPPON LIFE INDIA AMLTD#NIPPON INDIA MF-NIPPON INDIA ETF GOLDBEES"
            // or "MOTILAL OSWAL AMCLTD#MOTILAL OSWAL MF-MOTILAL OSWAL NASDAQ 100ETF"),
            // the actual instrument name is what follows '#', stripped of the AMC prefix if present.
            let mut etf_name = after_hash;
            if let Some(dash_idx) = etf_name.find('-') {
                etf_name = etf_name[dash_idx + 1..].trim();
            }
            cleaned = etf_name;
        } else {
            // Corporate action noise (e.g. "# NEW EQUITY SHARES WITH FV RS 2/- AFTER CONSOLIDATION")
            cleaned = cleaned[..idx].trim();
        }
    }
    // Share class suffix without a '#': "LIMITED - NEW EQUITY SHARES OF RE. 1/-",
    // "LIMITED-EQUITY SHARES", or run together as "MAHINDRAEQUITY SHARES".
    if let Some(m) = EQUITY_SUFFIX.find(cleaned) {
        if m.start() > 0 {
            cleaned = cleaned[..m.start()].trim();
        }
    }
    if let Some(idx) = cleaned.find("-NEW EQUITY") {
        cleaned = cleaned[..idx].trim();
    }
    if let Some(idx) = cleaned.find("Scheme Code") {
        cleaned = cleaned[..idx].trim();
    }

    // Insert spaces before commonly concatenated PDF words (e.g. "REFINERYLIMITED" -> "REFINERY LIMITED")
    let mut spaced = cleaned.to_string();
    for suffix in &[
        "LIMITED",
        "LTD",
        "COMPANY",
        "MANAGEMENT",
        "REFINERY",
        "HOLDINGS",
        "INDUSTRIES",
        "GROWTH",
        "DIRECT",
        "REGULAR",
        "PLAN",
        "FUND",
    ] {
        if spaced.ends_with(suffix) && spaced.len() > suffix.len() {
            let prefix = &spaced[..spaced.len() - suffix.len()];
            if !prefix.ends_with(' ') && !prefix.ends_with('-') {
                spaced = format!("{} {}", prefix, suffix);
            }
        }
        let pattern = format!("(?i)([a-z0-9])({})", suffix);
        if let Ok(re) = Regex::new(&pattern) {
            spaced = re.replace_all(&spaced, "$1 $2").to_string();
        }
    }

    // Replace multiple spaces or newlines with single space
    let words: Vec<&str> = spaced.split_whitespace().collect();
    words.join(" ")
}

/// Column order of the parser output. The `system_cdsl_cas` template maps
/// these by name.
pub const CDSL_HEADERS: [&str; 11] = [
    "Date",
    "Transaction Description",
    "Security Description",
    "ISIN",
    "Quantity",
    "Rate",
    "Amount",
    "BO ID",
    "Fee",
    "Avg Cost",
    "Particulars",
];

/// `Transaction Description` of a holding row; the import wizard uses it to
/// split holdings from transactions.
pub const HOLDING_BALANCE: &str = "HOLDING BALANCE";

/// `BO ID` of mutual fund rows held with the fund house (folios, not demat).
/// Folios held jointly get `MF FOLIOS (JOINT: <name>)`.
pub const MF_ACCOUNT: &str = "MF FOLIOS";

/// Labels written to `Transaction Description`. The template maps the first
/// five to activity types. The rest are left for the user to map or skip in
/// the review step, because the statement doesn't say what they mean for cost
/// (a bonus, split or merger, or an unknown credit or debit).
pub const LABEL_BUY: &str = "BUY";
pub const LABEL_SELL: &str = "SELL";
pub const LABEL_TRANSFER_IN: &str = "TRANSFER IN";
pub const LABEL_TRANSFER_OUT: &str = "TRANSFER OUT";
pub const LABEL_DIVIDEND: &str = "DIVIDEND";
pub const LABEL_CORPORATE_ACTION_CREDIT: &str = "CORPORATE ACTION CREDIT";
pub const LABEL_CORPORATE_ACTION_DEBIT: &str = "CORPORATE ACTION DEBIT";
pub const LABEL_DEMAT_CREDIT: &str = "DEMAT CREDIT";
pub const LABEL_DEMAT_DEBIT: &str = "DEMAT DEBIT";
pub const LABEL_MF_OTHER: &str = "MF TRANSACTION";

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// Label for a demat transaction from its particulars and direction.
/// `None` means the row doesn't change ownership (pledges, freezes) and is
/// left out.
pub fn demat_label(particulars: &str, credit: bool) -> Option<&'static str> {
    let u = particulars.to_uppercase();
    let pick = |cr: &'static str, dr: &'static str| Some(if credit { cr } else { dr });

    if contains_any(
        &u,
        &["PLEDGE", "LOCK-IN", "LOCKIN", "FREEZE", "MRGN", "MARGIN"],
    ) && !contains_any(&u, &["INVOC", "INVOK"])
    {
        return None;
    }
    if u.starts_with("CA-")
        || u.starts_with("CA ")
        || contains_any(
            &u,
            &[
                "BONUS",
                "SPLIT",
                "SUB-DIV",
                "SUBDIV",
                "CORPORATE ACTION",
                "MERGER",
                "AMALGAMATION",
                "DEMERGER",
                "CONVERSION",
                "RIGHTS",
                "BUYBACK",
                "BUY BACK",
                "CONSOLIDATION",
                "REDEMPTION",
                "EXTINGUISH",
            ],
        )
    {
        return pick(LABEL_CORPORATE_ACTION_CREDIT, LABEL_CORPORATE_ACTION_DEBIT);
    }
    if credit && contains_any(&u, &["IPO", "ALLOT", "INITIAL PUBLIC"]) {
        return Some(LABEL_BUY);
    }
    if contains_any(
        &u,
        &[
            "PAYOUT",
            "PAY-OUT",
            "PAY OUT",
            "PAYIN",
            "PAY-IN",
            "PAY IN",
            "EP-DR",
            "EARLY PAY",
        ],
    ) {
        return pick(LABEL_BUY, LABEL_SELL);
    }
    if contains_any(
        &u,
        &[
            "INTER DEPOSITORY",
            "INTER-DEPOSITORY",
            "INTDEP",
            "IDT",
            "OFF MARKET",
            "OFF-MARKET",
            "OFFMARKET",
            "OFF MKT",
            "TRANSFER",
            "TRF",
            "GIFT",
            "TRANSMISSION",
            "DEMAT",
            "REMAT",
        ],
    ) {
        return pick(LABEL_TRANSFER_IN, LABEL_TRANSFER_OUT);
    }
    if contains_any(&u, &["BUY", "SELL", "SALE", "SETT"]) {
        return pick(LABEL_BUY, LABEL_SELL);
    }
    pick(LABEL_DEMAT_CREDIT, LABEL_DEMAT_DEBIT)
}

/// Label for a mutual fund transaction. `None` for rows that aren't one,
/// such as a stamp duty line.
pub fn mf_label(description: &str, units_negative: bool) -> Option<&'static str> {
    let u = description.to_uppercase();
    if u.contains("STAMP DUTY") && !u.contains("PURCHASE") {
        return None;
    }
    let label = if contains_any(
        &u,
        &[
            "REDEMPTION",
            "REDEEM",
            "SWITCH OUT",
            "SWITCH-OUT",
            "SWITCHOUT",
            "SWP",
            "SYSTEMATIC WITHDRAWAL",
            "STP OUT",
            "STP-OUT",
            "TRANSFER OUT",
            "SELL",
            "SALE",
        ],
    ) {
        Some(LABEL_SELL)
    } else if u.contains("REINVEST") {
        Some(LABEL_BUY)
    } else if contains_any(&u, &["IDCW", "DIVIDEND"]) {
        Some(LABEL_DIVIDEND)
    } else if contains_any(
        &u,
        &[
            "PURCHASE",
            "SIP",
            "SYSTEMATIC INVESTMENT",
            "SWITCH IN",
            "SWITCH-IN",
            "SWITCHIN",
            "STP IN",
            "STP-IN",
            "NFO",
            "BUY",
            "ALLOTMENT",
            "SUBSCRIPTION",
            "TRANSFER IN",
        ],
    ) {
        Some(LABEL_BUY)
    } else {
        None
    };
    match label {
        Some(LABEL_BUY) | None if units_negative => Some(LABEL_SELL),
        other => other,
    }
}

/// A table cell: `Some(None)` for an empty `--`, `Some(Some(n))` for a
/// number, `None` when the text isn't a number. Handles Indian digit
/// grouping (`1,28,004.07`), a bare leading point (`.25`) and `(123)`
/// negatives.
fn number_cell(text: &str) -> Option<Option<Decimal>> {
    let t = text.trim();
    if t.chars().all(|c| c == '-') && t.len() >= 2 {
        return Some(None);
    }
    let (negative, body) = match t.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(inner) => (true, inner),
        None => (false, t),
    };
    let mut cleaned = body.replace(',', "");
    if cleaned.is_empty() {
        return None;
    }
    if let Some(rest) = cleaned.strip_prefix('.') {
        cleaned = format!("0.{rest}");
    } else if let Some(rest) = cleaned.strip_prefix("-.") {
        cleaned = format!("-0.{rest}");
    }
    let value = Decimal::from_str(&cleaned).ok()?;
    Some(Some(if negative { -value } else { value }))
}

fn number(text: &str) -> Option<Decimal> {
    number_cell(text).flatten()
}

fn fmt(d: Decimal) -> String {
    d.normalize().to_string()
}

fn fmt_opt(d: Option<Decimal>) -> String {
    d.map(fmt).unwrap_or_default()
}

/// Opening, credit, debit, closing and stamp duty of one demat row.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DematCells {
    opening: Option<Decimal>,
    credit: Option<Decimal>,
    debit: Option<Decimal>,
    closing: Option<Decimal>,
    stamp: Option<Decimal>,
}

impl DematCells {
    fn balances(&self) -> bool {
        match (self.opening, self.closing) {
            (Some(op), Some(cl)) => {
                op + self.credit.unwrap_or_default() - self.debit.unwrap_or_default() == cl
            }
            _ => false,
        }
    }
}

/// Reads the cells after a demat transaction date. CDSL prints Op. Bal,
/// Credit, Debit, Cl. Bal and Stamp Duty, but leaves out Op. Bal on the
/// second and later rows of a security, where it equals the last closing
/// balance. With four cells, the reading that balances wins.
fn demat_cells(values: &[Option<Decimal>], last_closing: Option<Decimal>) -> Option<DematCells> {
    let carried = |v: &[Option<Decimal>]| DematCells {
        opening: last_closing,
        credit: v[0],
        debit: v[1],
        closing: v[2],
        stamp: v.get(3).copied().flatten(),
    };
    match values.len() {
        n if n >= 5 => Some(DematCells {
            opening: values[0],
            credit: values[1],
            debit: values[2],
            closing: values[3],
            stamp: values[4],
        }),
        4 => {
            let with_opening = DematCells {
                opening: values[0],
                credit: values[1],
                debit: values[2],
                closing: values[3],
                stamp: None,
            };
            let carried = carried(values);
            if with_opening.balances() && !carried.balances() {
                Some(with_opening)
            } else {
                Some(carried)
            }
        }
        3 => Some(carried(values)),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn txn_row(
    date: &str,
    label: &str,
    security: &str,
    isin: &str,
    quantity: String,
    rate: String,
    amount: String,
    boid: &str,
    fee: String,
    particulars: &str,
) -> Vec<String> {
    vec![
        date.to_string(),
        label.to_string(),
        security.to_string(),
        isin.to_string(),
        quantity,
        rate,
        amount,
        boid.to_string(),
        fee,
        String::new(),
        particulars.to_string(),
    ]
}

/// Parse CDSL e-CAS text into tabular headers and rows (see [`CDSL_HEADERS`]).
///
/// - Holdings (demat and mutual fund) become `HOLDING BALANCE` rows dated on
///   the statement date. `Rate` is the market price or NAV; `Avg Cost` is set
///   for mutual funds only, from the cumulative amount invested, since CDSL
///   gives no cost for demat holdings.
/// - Demat transactions give quantities only: the quantity comes from the
///   Credit or Debit column, and the price is left for a broker trade list.
/// - Mutual fund transactions carry amount, NAV, price, units and stamp duty
///   (written to `Fee`, as it is part of the purchase cost).
/// - NPS rows are left out, as they have no ISIN.
pub fn parse_cdsl_cas(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = CDSL_HEADERS.iter().map(|h| h.to_string()).collect();

    let mut txn_rows: Vec<Vec<String>> = Vec::new();
    let mut holdings_rows: Vec<Vec<String>> = Vec::new();
    let mut current_boid = String::new();
    // Mutual fund folios aren't held in a demat account, so their rows get
    // their own account key instead of the last BO ID. A CAS can hold a
    // second investor section (folios held jointly), named in each page
    // header; those folios get a separate key.
    let mut in_mf_section = false;
    let mut joint_holder: Option<String> = None;
    let mut statement_date = String::new();
    let mut in_transaction_section = false;
    // Security of the transaction rows being read, and its last closing balance.
    let mut txn_isin = String::new();
    let mut txn_security = String::new();
    let mut last_closing: Option<Decimal> = None;

    let isin_regex = Regex::new(r"\b(IN[A-Z0-9]{10})\b").expect("hard-coded regex is valid");
    let date_regex =
        Regex::new(r"^\b(\d{1,2}[-/][A-Za-z]{3}[-/]\d{2,4}|\d{1,2}[-/]\d{1,2}[-/]\d{2,4})\b")
            .expect("hard-coded regex is valid");
    let boid_regex = Regex::new(r"\b(\d{16})\b").expect("hard-coded regex is valid");
    let mf_code_prefix_regex =
        Regex::new(r"^[A-Z0-9]{2,8}\s*-\s*").expect("hard-coded regex is valid");
    // A scheme line in the MF transaction table: "STDG - Axis Short Term Fund".
    let mf_scheme_line_regex =
        Regex::new(r"^[A-Z0-9]{2,8}\s+-\s+\S").expect("hard-coded regex is valid");

    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    // 0. Pre-scan Mutual Fund Folios section if present:
    // e.g.
    // Scheme Name : NIPPON INDIA SMALL CAP FUND - DIRECT GROWTH PLAN GROWTH OPTION
    // ...
    // ISIN : INF204K01K15
    let mut mf_scheme_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut scan_idx = 0;
    while scan_idx < lines.len() {
        let line = lines[scan_idx];
        if line.starts_with("Scheme Name") || line.contains("Scheme Name :") {
            let scheme_raw = line.split(':').nth(1).unwrap_or("").trim();
            let mut full_scheme = scheme_raw.to_string();
            // In case the scheme name wraps across lines before Folio No / ISIN
            let mut look = 1;
            while scan_idx + look < lines.len() && look <= 3 {
                let next_l = lines[scan_idx + look];
                if next_l.starts_with("Folio No")
                    || next_l.starts_with("ISIN")
                    || next_l.starts_with("AMC Name")
                    || next_l.starts_with("Scheme Name")
                {
                    break;
                }
                if !next_l.is_empty() && !is_summary_or_noise_line(next_l) {
                    full_scheme.push(' ');
                    full_scheme.push_str(next_l);
                }
                look += 1;
            }

            // Look forward for ISIN within next 10 lines
            for isin_look in 1..=10 {
                if scan_idx + isin_look >= lines.len() {
                    break;
                }
                let cand_l = lines[scan_idx + isin_look];
                if cand_l.starts_with("Scheme Name") || cand_l.starts_with("AMC Name") {
                    break;
                }
                if let Some(c) = isin_regex.captures(cand_l) {
                    let found_isin = c.get(1).map_or("", |m| m.as_str()).to_string();
                    if !full_scheme.is_empty() {
                        mf_scheme_map.insert(found_isin, clean_security_description(&full_scheme));
                    }
                    break;
                }
            }
        }
        scan_idx += 1;
    }

    let date_match_re = Regex::new(r"\b(\d{1,2}[-/][A-Za-z0-9]{2,3}[-/]\d{2,4})\b")
        .expect("hard-coded regex is valid");
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        // Page header: the investor section is named again on each page.
        if line.contains("CONSOLIDATED ACCOUNT STATEMENT") {
            joint_holder = None;
        }
        if let Some((_, name)) = line.split_once("Joint A/C Holder:") {
            let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
            joint_holder = (!name.is_empty()).then_some(name);
        }
        {
            let lower = line.to_lowercase();
            if lower.contains("mutual fund units held") {
                in_mf_section = true;
            } else if lower.contains("bo id")
                || lower.contains("dp name")
                || lower.contains("demat accounts held")
                || lower.contains("nps pran")
            {
                in_mf_section = false;
            }
        }
        let account_key = if in_mf_section {
            match &joint_holder {
                Some(name) => format!("{MF_ACCOUNT} (JOINT: {})", name.to_uppercase()),
                None => MF_ACCOUNT.to_string(),
            }
        } else {
            current_boid.clone()
        };

        // Skip lines with Hindi or portfolio summary markers
        if is_summary_or_noise_line(line) {
            i += 1;
            continue;
        }

        let lower = line.to_lowercase();

        // Detect section headers: "HOLDING STATEMENT" vs "STATEMENT OF TRANSACTIONS"
        if lower.contains("statement of transactions") || lower.contains("transaction statement") {
            in_transaction_section = true;
            txn_isin.clear();
            txn_security.clear();
            last_closing = None;
        } else if lower.contains("holding statement")
            || lower.contains("statement of holdings")
            || lower.contains("holding as on")
            || lower.contains("units held as on")
        {
            in_transaction_section = false;
        }

        // 1. Detect BO ID (16 digits)
        if lower.contains("bo id") || lower.contains("boid") || lower.contains("demat account") {
            if let Some(caps) = boid_regex.captures(line) {
                current_boid = caps.get(1).map_or("", |m| m.as_str()).to_string();
            } else if i + 1 < lines.len() {
                if let Some(caps) = boid_regex.captures(lines[i + 1]) {
                    current_boid = caps.get(1).map_or("", |m| m.as_str()).to_string();
                }
            }
        }

        // 2. Detect statement date (e.g. "HOLDING STATEMENT AS ON 31-08-2026")
        // The end of "FOR THE PERIOD FROM 01-08-2026 TO 31-08-2026" is the
        // same date, so it is used when it comes first.
        if (lower.contains("statement as on")
            || lower.contains("held as on")
            || lower.contains("as on date")
            || lower.contains("statement date"))
            && statement_date.is_empty()
        {
            if let Some(m) = date_match_re.find(line) {
                statement_date = m.as_str().to_string();
            }
        } else if lower.contains("for the period from") && statement_date.is_empty() {
            // Needs both dates: a title that wraps before "TO" has only the start.
            let dates: Vec<_> = date_match_re.find_iter(line).collect();
            if let [_, .., end] = dates.as_slice() {
                statement_date = end.as_str().to_string();
            }
        }

        // 3. Detect ISIN row (Holdings section - ignore if currently inside transaction section!)
        if !in_transaction_section {
            if let Some(caps) = isin_regex.captures(line) {
                let isin = caps.get(1).map_or("", |m| m.as_str()).to_string();

                // Determine security / scheme name
                let mut sec_name = String::new();
                if let Some(mapped_name) = mf_scheme_map.get(&isin) {
                    sec_name = mapped_name.clone();
                }

                // Check if preceding line has a mutual fund scheme name (e.g. "SCAG - NIPPON INDIA...")
                if sec_name.is_empty() && i > 0 {
                    let prev_line = lines[i - 1];
                    let prev_lower = prev_line.to_lowercase();
                    if (prev_lower.contains("fund")
                        || prev_lower.contains("growth")
                        || prev_lower.contains("direct")
                        || prev_lower.contains("regular")
                        || prev_lower.contains("plan"))
                        && !isin_regex.is_match(prev_line)
                        && !is_summary_or_noise_line(prev_line)
                    {
                        let stripped = mf_code_prefix_regex.replace(prev_line, "").to_string();
                        sec_name = clean_security_description(&stripped);
                    }
                }

                // Check if next lines contain demat security name or folio number
                let mut sec_name_parts = Vec::new();
                let mut offset = 1;
                let mut is_folio_line = false;

                if i + 1 < lines.len() {
                    let next_line = lines[i + 1].trim();
                    // A folio number consists only of digits and optional slash (e.g. "123456789012/0" or "1234567890/0")
                    let is_pure_folio = !next_line.is_empty()
                        && next_line
                            .chars()
                            .all(|c| c.is_ascii_digit() || c == '/' || c == '-')
                        && next_line.chars().any(|c| c.is_ascii_digit())
                        && !next_line.chars().any(|c| c.is_alphabetic());

                    if is_pure_folio && (next_line.contains('/') || next_line.len() >= 6) {
                        is_folio_line = true;
                        offset = 2; // Skip folio line for number extraction
                    }
                }

                if sec_name.is_empty() && !is_folio_line {
                    while i + offset < lines.len() && offset <= 4 {
                        let next_line = lines[i + offset];
                        if isin_regex.is_match(next_line)
                            || next_line.starts_with("BO ID")
                            || next_line.contains("HOLDING STATEMENT")
                            || is_summary_or_noise_line(next_line)
                        {
                            break;
                        }
                        if number_cell(next_line).is_some() {
                            // Reached the cells (Current Bal, "--", Market Price, ...)
                            break;
                        }
                        let cleaned_part = clean_security_description(next_line);
                        if !cleaned_part.is_empty() {
                            sec_name_parts.push(cleaned_part);
                        }
                        offset += 1;
                    }
                    sec_name = clean_security_description(&sec_name_parts.join(" "));
                }

                let mut found_qty = None;
                let mut found_rate = None;
                let mut found_val = None;
                let mut found_avg_cost = None;

                let mut num_cands = Vec::new();
                // Every cell, "--" included, in order.
                let mut cells: Vec<Option<(f64, String)>> = Vec::new();
                for num_offset in offset..=offset + 10 {
                    if i + num_offset >= lines.len() {
                        break;
                    }
                    let cand = lines[i + num_offset];
                    if isin_regex.is_match(cand)
                        || cand.starts_with("BO ID")
                        || cand.contains("HOLDING STATEMENT")
                    {
                        break;
                    }
                    if is_summary_or_noise_line(cand) {
                        continue;
                    }
                    match number_cell(cand) {
                        Some(Some(value)) => {
                            let clean_num = cand.replace(',', "");
                            if let Ok(num) = clean_num.parse::<f64>() {
                                num_cands.push((num, clean_num.clone()));
                            }
                            // Keep the cell even when f64 can't read it, so
                            // later cells stay in their columns.
                            cells.push(Some((
                                clean_num.parse::<f64>().unwrap_or_default(),
                                fmt(value),
                            )));
                        }
                        Some(None) => cells.push(None),
                        None => {}
                    }
                }

                // In CDSL Demat Table:
                // CurrentBal (cand 0), Free Bal (cand 1), MarketPrice (cand 2), Value (cand 3)
                // In CDSL MF Table (after folio):
                // Units (cand 0), NAV (cand 1), Cost (cand 2), Value (cand 3)
                // A demat holding row has seven cells: Current Bal, Frozen,
                // Pledge, Pledge Setup and Free Bal, then Market Price and
                // Value. Read them by column: a pledged or frozen holding has
                // numbers in the middle columns, and a security sold out during
                // the month has "--" in every balance (zero, not its price).
                let cell_text = |c: &Option<(f64, String)>| c.as_ref().map(|(_, t)| t.clone());
                if !is_folio_line && cells.len() >= 7 {
                    found_qty = Some(cell_text(&cells[0]).unwrap_or_else(|| "0".to_string()));
                    found_rate = cell_text(&cells[5]);
                    found_val = cell_text(&cells[6]);
                } else if !num_cands.is_empty() && num_cands[0].0 > 0.0 {
                    found_qty = Some(num_cands[0].1.clone());
                    if num_cands.len() >= 4 {
                        if is_folio_line {
                            // MF Table: Cand 0 = Units, Cand 1 = NAV, Cand 3 = Value
                            found_rate = Some(num_cands[1].1.clone());
                            found_val = Some(num_cands[3].1.clone());
                            // Cand 2 = cumulative amount invested.
                            found_avg_cost =
                                match (number(&num_cands[2].1), number(&num_cands[0].1)) {
                                    (Some(invested), Some(units)) if !units.is_zero() => {
                                        Some(fmt((invested / units).round_dp(4)))
                                    }
                                    _ => None,
                                };
                        } else {
                            // Demat Table: Cand 0 = Qty, Cand 2 = Market Price, Cand 3 = Value
                            found_rate = Some(num_cands[2].1.clone());
                            found_val = Some(num_cands[3].1.clone());
                        }
                    } else if num_cands.len() >= 3 {
                        found_rate = Some(num_cands[1].1.clone());
                        found_val = Some(num_cands[2].1.clone());
                    } else if num_cands.len() >= 2 {
                        found_rate = Some(num_cands[1].1.clone());
                    }
                }

                if let Some(qty) = found_qty {
                    let h_date = if !statement_date.is_empty() {
                        statement_date.clone()
                    } else {
                        chrono::Utc::now().format("%d-%b-%Y").to_string()
                    };

                    holdings_rows.push(vec![
                        h_date,
                        HOLDING_BALANCE.to_string(),
                        sec_name,
                        isin.clone(),
                        qty,
                        found_rate.unwrap_or_default(),
                        found_val.unwrap_or_default(),
                        account_key.clone(),
                        String::new(),
                        found_avg_cost.unwrap_or_default(),
                        String::new(),
                    ]);
                }
            }
        }

        // 4. Transactions. Track the security the rows belong to.
        {
            if let Some(caps) = isin_regex.captures(line) {
                let isin = caps.get(1).map_or("", |m| m.as_str()).to_string();
                if isin != txn_isin {
                    last_closing = None;
                }
                let inline = line
                    .replace(&isin, "")
                    .replace("ISIN:", "")
                    .replace("ISIN :", "")
                    .replace("ISIN", "");
                let inline = inline.trim();
                txn_security = if inline.len() > 3 && !is_summary_or_noise_line(inline) {
                    clean_security_description(inline)
                } else if let Some(name) = mf_scheme_map.get(&isin) {
                    name.clone()
                } else if i > 0 && mf_scheme_line_regex.is_match(lines[i - 1]) {
                    let stripped = mf_code_prefix_regex.replace(lines[i - 1], "");
                    clean_security_description(&stripped)
                } else if line == isin
                    && i + 1 < lines.len()
                    && !date_regex.is_match(lines[i + 1])
                    && number_cell(lines[i + 1]).is_none()
                    && !is_summary_or_noise_line(lines[i + 1])
                {
                    // Demat table: the security follows a bare ISIN.
                    clean_security_description(lines[i + 1])
                } else {
                    String::new()
                };
                txn_isin = isin;
                if in_transaction_section {
                    i += 1;
                    continue;
                }
            }
        }

        if let Some(date_match) = date_regex.find(line) {
            let date_str = date_match.as_str().to_string();
            let remainder = line[date_match.end()..].trim();

            if remainder.is_empty() && in_transaction_section && !txn_isin.is_empty() {
                let next = lines.get(i + 1).copied().unwrap_or("");
                if number_cell(next).is_some() {
                    // Demat row: particulars come before the date, cells after.
                    let mut values = Vec::new();
                    let mut j = i + 1;
                    while j < lines.len() && values.len() < 5 {
                        match number_cell(lines[j]) {
                            Some(v) => values.push(v),
                            None => break,
                        }
                        j += 1;
                    }
                    let particulars = if i > 0
                        && !isin_regex.is_match(lines[i - 1])
                        && number_cell(lines[i - 1]).is_none()
                        && !is_summary_or_noise_line(lines[i - 1])
                    {
                        lines[i - 1]
                    } else {
                        ""
                    };
                    if let Some(cells) = demat_cells(&values, last_closing) {
                        let moves = [(true, cells.credit), (false, cells.debit)];
                        for (credit, qty) in moves {
                            let Some(qty) = qty.filter(|q| !q.is_zero()) else {
                                continue;
                            };
                            let Some(label) = demat_label(particulars, credit) else {
                                continue;
                            };
                            txn_rows.push(txn_row(
                                &date_str,
                                label,
                                &txn_security,
                                &txn_isin,
                                fmt(qty.abs()),
                                String::new(),
                                String::new(),
                                &account_key,
                                fmt_opt(cells.stamp.filter(|s| !s.is_zero())),
                                particulars,
                            ));
                        }
                        last_closing = cells.closing.or(last_closing);
                    }
                    i = j;
                    continue;
                }

                // Mutual fund row: description, then Amount, NAV, Price,
                // Units, Stamp Duty (older layouts: Amount, NAV, Units).
                if number_cell(next).is_none() && !is_summary_or_noise_line(next) {
                    let mut values = Vec::new();
                    let mut j = i + 2;
                    while j < lines.len() && values.len() < 7 {
                        match number_cell(lines[j]) {
                            Some(v) => values.push(v),
                            None => break,
                        }
                        j += 1;
                    }
                    let (amount, nav, price, units, stamp) = match values.len() {
                        n if n >= 4 => (
                            values[0],
                            values[1],
                            values[2],
                            values[3],
                            values.get(4).copied().flatten(),
                        ),
                        3 => (values[0], values[1], None, values[2], None),
                        2 => (values[0], None, None, values[1], None),
                        _ => (None, None, None, None, None),
                    };
                    let units_negative = units.is_some_and(|u| u.is_sign_negative())
                        || amount.is_some_and(|a| a.is_sign_negative());
                    if let Some(label) = mf_label(next, units_negative) {
                        let has_units = units.is_some_and(|u| !u.is_zero());
                        if has_units || label == LABEL_DIVIDEND {
                            txn_rows.push(txn_row(
                                &date_str,
                                label,
                                &txn_security,
                                &txn_isin,
                                units.map(|u| fmt(u.abs())).unwrap_or_default(),
                                fmt_opt(price.filter(|p| !p.is_zero()).or(nav)),
                                amount.map(|a| fmt(a.abs())).unwrap_or_default(),
                                &account_key,
                                fmt_opt(stamp.filter(|s| !s.is_zero())),
                                next,
                            ));
                        }
                    }
                    if !values.is_empty() {
                        i = j;
                        continue;
                    }
                }
            } else if !remainder.is_empty() && !is_summary_or_noise_line(remainder) {
                // Single-line row: "10-May-2024 BUY 10 2850.50 28505.00".
                let tokens: Vec<&str> = remainder.split_whitespace().collect();
                let first_num = tokens.iter().position(|t| number(t).is_some());
                if let Some(first_num) = first_num.filter(|&n| n > 0) {
                    let desc = tokens[..first_num].join(" ");
                    let nums: Vec<Decimal> = tokens[first_num..]
                        .iter()
                        .filter_map(|t| number(t))
                        .collect();
                    let qty = nums.first().copied();
                    let negative = qty.is_some_and(|q| q.is_sign_negative());
                    if let (Some(label), Some(qty)) = (mf_label(&desc, negative), qty) {
                        let (isin, security) = if txn_isin.is_empty() {
                            (String::new(), String::new())
                        } else {
                            (txn_isin.clone(), txn_security.clone())
                        };
                        txn_rows.push(txn_row(
                            &date_str,
                            label,
                            &security,
                            &isin,
                            fmt(qty.abs()),
                            fmt_opt(nums.get(1).copied()),
                            fmt_opt(nums.get(2).map(|a| a.abs())),
                            &account_key,
                            String::new(),
                            &desc,
                        ));
                    }
                }
            }
        }

        i += 1;
    }

    // Transactions first, then holdings.
    let mut combined = txn_rows;
    combined.extend(holdings_rows);

    (headers, combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_name_uses_statement_scheme_for_same_isin() {
        let sample = "Scheme Name : Example Small Cap Fund - Direct Growth\nISIN : INF000000001\nSTATEMENT OF TRANSACTIONS\nISIN : INF000000001\n10-May-2024 BUY 2 10 20\nISIN : INF000000002\n11-May-2024 BUY 3 10 30\n";
        let (_, rows) = parse_cdsl_cas(sample);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][2], "Example Small Cap Fund - Direct Growth");
        assert_eq!(rows[1][2], "");
        assert_eq!(rows[1][3], "INF000000002");
    }

    #[test]
    fn wrapped_transaction_name_uses_scheme_defined_later() {
        let sample = "STATEMENT OF TRANSACTIONS\nISIN : INF000000001\n10-May-2024\nPurchase\n20\n10\n2\nAMC Name : Example Mutual Fund\nScheme Name : Example Small Cap Fund - Direct Growth\nISIN : INF000000001\n";
        let (_, rows) = parse_cdsl_cas(sample);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][2], "Example Small Cap Fund - Direct Growth");
    }

    #[test]
    fn transaction_name_preserves_inline_description() {
        let sample = "Scheme Name : Example Fund - Direct Growth\nISIN : INF000000001\nSTATEMENT OF TRANSACTIONS\nISIN: INF000000001 Existing Description\n10-May-2024 BUY 2 10 20\n";
        let (_, rows) = parse_cdsl_cas(sample);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][2], "Existing Description");
    }

    #[test]
    fn test_is_hindi_detection() {
        assert!(is_hindi_text("तक के अनुसार सभी नवेश म कुल पोटफोलयो मूय ₹"));
        assert!(!is_hindi_text("RELIANCE INDUSTRIES LIMITED"));
    }

    #[test]
    fn test_clean_security_description() {
        let raw = "BSE LIMITED # NEW EQUITYSHARES WITH FV RS 2/- AFTERCONSOLIDATION";
        assert_eq!(clean_security_description(raw), "BSE LIMITED");

        let raw2 = "THE TATA POWER COMPANYLIMITED-NEW EQUITY SHARESOF RE.1/- AFTER SUB-DIVISION";
        assert_eq!(
            clean_security_description(raw2),
            "THE TATA POWER COMPANY LIMITED"
        );

        let raw_goldbees = "NIPPON LIFE INDIA AMLTD#NIPPON INDIA MF-NIPPON INDIA ETF GOLDBEES";
        assert_eq!(
            clean_security_description(raw_goldbees),
            "NIPPON INDIA ETF GOLDBEES"
        );

        let raw_hdfc = "HDFC ASSET MANAGEMENTCOMPANY LIMITED";
        assert_eq!(
            clean_security_description(raw_hdfc),
            "HDFC ASSET MANAGEMENT COMPANY LIMITED"
        );
    }

    #[test]
    fn test_parse_cdsl_sample() {
        let sample = r#"
BO ID: 1208160012345678
ISIN: INE002A01018 RELIANCE INDUSTRIES LTD
10-May-2024 BUY 10 2850.50 28505.00
12-May-2024 SALE 5 2900.00 14500.00
31-08-2026 तक के अनुसार सभी नवेश म कुल पोटफोलयो मूय ₹ 7110492.37
"#;
        let (headers, rows) = parse_cdsl_cas(sample);
        assert_eq!(headers.len(), CDSL_HEADERS.len());
        // Hindi portfolio summary line must be excluded!
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "10-May-2024");
        assert_eq!(rows[0][1], LABEL_BUY);
        assert_eq!(rows[0][3], "INE002A01018");
        assert_eq!(rows[1][0], "12-May-2024");
        assert_eq!(rows[1][1], LABEL_SELL);
        assert_eq!(rows[1][4], "5");
    }

    #[test]
    fn test_parse_cdsl_mf_holdings() {
        let sample = r#"
BO ID: 1208160000000009
HOLDING STATEMENT AS ON 31-08-2026

AMC Name : Nippon India Mutual Fund
Folio No : 123456789012/0
Scheme Name : NIPPON INDIA SMALL CAP FUND - DIRECT GROWTH PLAN GROWTH OPTION
ISIN : INF204K01K15

SCAG - NIPPON INDIA SMALL CAP FUND - DIRECT GROWTH PLAN GROWTH OPTION
INF204K01K15
123456789012/0
1000.5
210.6366
169000.00
219669.33
"#;
        let (_headers, rows) = parse_cdsl_cas(sample);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row[0], "31-08-2026");
        assert_eq!(row[1], "HOLDING BALANCE");
        assert_eq!(
            row[2],
            "NIPPON INDIA SMALL CAP FUND - DIRECT GROWTH PLAN GROWTH OPTION"
        );
        assert_eq!(row[3], "INF204K01K15");
        assert_eq!(row[4], "1000.5");
        assert_eq!(row[5], "210.6366");
        assert_eq!(row[6], "219669.33");
        assert_eq!(row[7], "1208160000000009");
    }

    /// Demat transaction table as text extraction lays it out: one cell per
    /// line, particulars before the date, Op. Bal left out after the first row.
    const DEMAT_TXNS: &str = "BO ID: 1208160000000001
STATEMENT OF TRANSACTIONS FOR THE PERIOD FROM 01-08-2026 TO 31-08-2026
ISIN
Security
TransactionParticulars
Date
Op. Bal
Credit
Debit
Cl. Bal
StampDuty(`)
INF000000AA1
EXAMPLE AMC LTD#EXAMPLE MF-EXAMPLE ETF GOLDBEES
PAYOUT-CR CM00001 TM/CP00001 SETT0000000001
12-08-2026
585.000
65.000
--
650.000
0
PAYOUT-CR CM00001 TM/CP00001 SETT0000000002
14-08-2026
50.000
--
700.000
0
INE000000AA2
EXAMPLE INDUSTRIES LIMITED
EP-DR CM00001 SETT0000000003
20-08-2026
100.000
--
40.000
60.000
0
MRGN PLEDGE SETUP 1234
21-08-2026
--
10.000
50.000
0
BONUS CA-123456
22-08-2026
60.000
--
120.000
0
";

    #[test]
    fn demat_rows_take_quantity_from_credit_and_debit() {
        let (headers, rows) = parse_cdsl_cas(DEMAT_TXNS);
        assert_eq!(headers.len(), CDSL_HEADERS.len());
        let summary: Vec<(&str, &str, &str, &str)> = rows
            .iter()
            .map(|r| (r[0].as_str(), r[1].as_str(), r[3].as_str(), r[4].as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("12-08-2026", LABEL_BUY, "INF000000AA1", "65"),
                ("14-08-2026", LABEL_BUY, "INF000000AA1", "50"),
                ("20-08-2026", LABEL_SELL, "INE000000AA2", "40"),
                (
                    "22-08-2026",
                    LABEL_CORPORATE_ACTION_CREDIT,
                    "INE000000AA2",
                    "60"
                ),
            ]
        );
        assert_eq!(rows[0][2], "EXAMPLE ETF GOLDBEES");
        assert_eq!(rows[2][2], "EXAMPLE INDUSTRIES LIMITED");
        // No prices in a depository statement.
        assert!(rows.iter().all(|r| r[5].is_empty() && r[6].is_empty()));
        assert_eq!(rows[0][7], "1208160000000001");
        assert_eq!(rows[0][10], "PAYOUT-CR CM00001 TM/CP00001 SETT0000000001");
    }

    #[test]
    fn mf_rows_carry_price_units_and_stamp_duty_and_nps_is_skipped() {
        let sample = "STATEMENT OF TRANSACTIONS FOR THE PERIOD FROM 01-08-2026 TO 31-08-2026
Date
Scheme Details
Opening Balance
5,900.1111
03-Aug-2026
EXAMPLE PENSION FUND SCHEME E- TIER I
By Contribution forJuly,2026
CR
6,879.75
76.0711
90.4384
Closing Balance
5,990.5495
STATEMENT OF TRANSACTIONS FOR THE PERIOD FROM 01-08-2026 TO 31-08-2026
Example Mutual Fund
STDG - Example Short Term Fund - Direct Growth
ISIN : INF000000BB1
UCC : MFEX0001
Date
Transaction Description
Opening Balance
5548.065
03-08-2026
Systematic Investment(NAVDt : 03/08/2026) (1/Perpetual)  25658431
4999.75
36.1677
36.1677
138.238
.25
0
0
10-08-2026
Redemption
(1,000.00)
36.2000
36.2000
(27.624)
0
0
0
Closing Balance
5658.679
";
        let (_, rows) = parse_cdsl_cas(sample);
        assert_eq!(rows.len(), 2, "{rows:?}");
        let buy = &rows[0];
        assert_eq!(buy[1], LABEL_BUY);
        assert_eq!(buy[2], "Example Short Term Fund - Direct Growth");
        assert_eq!(buy[3], "INF000000BB1");
        assert_eq!(buy[4], "138.238");
        assert_eq!(buy[5], "36.1677");
        assert_eq!(buy[6], "4999.75");
        assert_eq!(buy[8], "0.25");
        let sell = &rows[1];
        assert_eq!(sell[0], "10-08-2026");
        assert_eq!(sell[1], LABEL_SELL);
        assert_eq!(sell[4], "27.624");
        assert_eq!(sell[6], "1000");
        assert_eq!(sell[8], "");
    }

    #[test]
    fn mf_holdings_after_a_transaction_section_are_read_with_avg_cost() {
        let sample = "STATEMENT OF TRANSACTIONS FOR THE PERIOD FROM 01-08-2026 TO 31-08-2026
No Transaction during the period
MUTUAL FUND UNITS HELD AS ON 31-08-2026
Scheme Name
ISIN
Folio No.
ClosingBal(Units)
NAV (`)
CumulativeAmountInvested (inINR)
Valuation (`)
860 - Example HealthcareFund - Dir - Growth
INF000000CC1
1234567/89
2000
51.693
50,000.00
1,03,386.00
53,386.00
106.77
";
        let (_, rows) = parse_cdsl_cas(sample);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let row = &rows[0];
        assert_eq!(row[0], "31-08-2026");
        assert_eq!(row[1], HOLDING_BALANCE);
        assert_eq!(row[3], "INF000000CC1");
        assert_eq!(row[4], "2000");
        assert_eq!(row[5], "51.693");
        assert_eq!(row[6], "103386.00");
        // 50,000.00 / 2,000
        assert_eq!(row[9], "25");
    }

    #[test]
    fn a_sold_out_demat_holding_reads_as_zero() {
        let sample = "BO ID: 1208160000000001
HOLDING STATEMENT AS ON 31-10-2026
INE000000AA1
EXAMPLE SYSTEMS LIMITED #EQUITY SHARES
--
--
--
--
--
204.7500
0.00
INE000000BB2
EXAMPLE POWER LIMITED - NEWEQUITY SHARES OF RE. 2/-AFTER SPLIT
100.000
--
--
--
100.000
800.0000
80,000.00
INE000000CC3
EXAMPLE PLEDGED LIMITED
100.000
--
40.000
--
60.000
800.0000
80,000.00
IN0020200161
GOVT OF INDIA#2.5% SGB2020-21 SERIES V 2028-LOANCODE 29278 - RED DT11.08.2028
1.000
--
--
--
1.000
14007.0000
14,007.00
";
        let (_, rows) = parse_cdsl_cas(sample);
        let summary: Vec<(&str, &str, &str, &str)> = rows
            .iter()
            .map(|r| (r[2].as_str(), r[4].as_str(), r[5].as_str(), r[6].as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("EXAMPLE SYSTEMS LIMITED", "0", "204.75", "0"),
                ("EXAMPLE POWER LIMITED", "100", "800", "80000"),
                ("EXAMPLE PLEDGED LIMITED", "100", "800", "80000"),
                ("2.5% SGB2020-21 SERIES V 2028", "1", "14007", "14007"),
            ]
        );
    }

    #[test]
    fn mf_rows_get_their_own_account_key_per_investor_section() {
        let sample = "CONSOLIDATED ACCOUNT STATEMENT (CAS) FOR SECURITIES HELD IN DEMAT
DP Name : EXAMPLE BROKING LIMITED
BO ID : 1208160000000001
HOLDING STATEMENT AS ON 31-10-2026
INE000000BB2
EXAMPLE POWER LIMITED
100.000
--
--
--
100.000
800.0000
80,000.00
CONSOLIDATED ACCOUNT STATEMENT (CAS) FOR SECURITIES HELD IN DEMAT
FIRST HOLDER .
MUTUAL FUND UNITS HELD WITH MF/RTA
MUTUAL FUND UNITS HELD AS ON 31-10-2026
860 - Example HealthcareFund - Dir - Growth
INF000000CC1
1234567/89
2000
51.693
50,000.00
1,03,386.00
53,386.00
106.77
CONSOLIDATED ACCOUNT STATEMENT (CAS) FOR SECURITIES HELD IN DEMAT
First HolderJoint A/C Holder: SECOND  HOLDER
MUTUAL FUND UNITS HELD WITH MF/RTA
STATEMENT OF TRANSACTIONS FOR THE PERIOD FROM 01-10-2026 TO 31-10-2026
Example Mutual Fund
STDG - Example Short Term Fund - Direct Growth
ISIN : INF000000BB1
Opening Balance
5548.065
01-10-2026
Systematic Investment 25658431
4999.75
36.1677
36.1677
138.238
.25
0
0
Closing Balance
5686.303
MUTUAL FUND UNITS HELD AS ON 31-10-2026
STDG - Example Short Term Fund - Direct Growth
INF000000BB1
910238755965/0
5686.303
36.2658
2,15,000.00
2,06,220.80
1,220.80
0.57
";
        let (_, rows) = parse_cdsl_cas(sample);
        let accounts: Vec<(&str, &str, &str)> = rows
            .iter()
            .map(|r| (r[1].as_str(), r[3].as_str(), r[7].as_str()))
            .collect();
        let joint = "MF FOLIOS (JOINT: SECOND HOLDER)";
        assert_eq!(
            accounts,
            vec![
                (LABEL_BUY, "INF000000BB1", joint),
                (HOLDING_BALANCE, "INE000000BB2", "1208160000000001"),
                (HOLDING_BALANCE, "INF000000CC1", MF_ACCOUNT),
                (HOLDING_BALANCE, "INF000000BB1", joint),
            ]
        );
    }

    #[test]
    fn a_wrapped_period_title_does_not_set_the_statement_date() {
        let sample = "CONSOLIDATED ACCOUNT STATEMENT FOR THE PERIOD FROM 01-08-2026
TO 31-08-2026
MUTUAL FUND UNITS HELD AS ON 31-08-2026
Scheme Name
ISIN
Folio No.
ClosingBal(Units)
NAV (`)
CumulativeAmountInvested (inINR)
Valuation (`)
860 - Example HealthcareFund - Dir - Growth
INF000000CC1
1234567/89
2000
51.693
50,000.00
1,03,386.00
53,386.00
106.77
";
        let (_, rows) = parse_cdsl_cas(sample);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0][0], "31-08-2026");
    }

    #[test]
    fn demat_labels() {
        assert_eq!(demat_label("PAYOUT-CR CM1 SETT1", true), Some(LABEL_BUY));
        assert_eq!(demat_label("EP-DR CM1 SETT1", false), Some(LABEL_SELL));
        assert_eq!(demat_label("PAYIN-DR SETT1", false), Some(LABEL_SELL));
        assert_eq!(demat_label("INTDEP-CR 1234", true), Some(LABEL_TRANSFER_IN));
        assert_eq!(
            demat_label("OFF MARKET TRANSFER", false),
            Some(LABEL_TRANSFER_OUT)
        );
        assert_eq!(demat_label("IPO ALLOTMENT", true), Some(LABEL_BUY));
        assert_eq!(
            demat_label("SPLIT CA-1234", false),
            Some(LABEL_CORPORATE_ACTION_DEBIT)
        );
        assert_eq!(demat_label("MARGIN PLEDGE", false), None);
        assert_eq!(
            demat_label("PLEDGE INVOCATION", false),
            Some(LABEL_DEMAT_DEBIT)
        );
        assert_eq!(
            demat_label("SOMETHING ELSE", true),
            Some(LABEL_DEMAT_CREDIT)
        );
    }

    #[test]
    fn mf_labels() {
        assert_eq!(
            mf_label("SIP Purchase Instalment No -7", false),
            Some(LABEL_BUY)
        );
        assert_eq!(
            mf_label("Systematic Investment 123", false),
            Some(LABEL_BUY)
        );
        assert_eq!(
            mf_label("Switch Out - To Example Fund", false),
            Some(LABEL_SELL)
        );
        assert_eq!(mf_label("IDCW Reinvestment", false), Some(LABEL_BUY));
        assert_eq!(mf_label("IDCW Paid", false), Some(LABEL_DIVIDEND));
        assert_eq!(mf_label("*** Stamp Duty ***", false), None);
        assert_eq!(mf_label("Unknown", true), Some(LABEL_SELL));
        assert_eq!(mf_label("Unknown", false), None);
    }

    #[test]
    fn number_cells() {
        assert_eq!(number_cell("--"), Some(None));
        assert_eq!(
            number_cell("1,28,004.07"),
            Some(Some(Decimal::new(12800407, 2)))
        );
        assert_eq!(number_cell(".25"), Some(Some(Decimal::new(25, 2))));
        assert_eq!(number_cell("(27.624)"), Some(Some(Decimal::new(-27624, 3))));
        assert_eq!(number_cell("Closing Balance"), None);
        assert_eq!(number_cell("-"), None);
    }
}
