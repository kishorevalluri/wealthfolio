//! CDSL e-CAS (Central Depository Services Limited) parser.
//!
//! Handles CDSL Consolidated Account Statements (CAS) containing both
//! Demat Equity & Mutual Fund Holdings and Monthly Transactions.
//!
//! Features:
//! - Filters out Hindi / Devanagari bilingual headers and portfolio total summary rows.
//! - Cleans verbose corporate action noise from security names.
//! - Extracts valid ISIN holding balances and monthly buy/sell transactions.

use regex::Regex;

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
        if after_upper.contains("ETF")
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
    if let Some(idx) = cleaned.find("-NEW EQUITY") {
        cleaned = cleaned[..idx].trim();
    }
    if let Some(idx) = cleaned.find("-EQUITY SHARES") {
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

/// Parse CDSL e-CAS text into tabular headers and rows.
///
/// Output columns matching `system_cdsl_cas`:
/// - `Date`
/// - `Transaction Description`
/// - `Security Description`
/// - `ISIN`
/// - `Quantity`
/// - `Rate`
/// - `Amount`
/// - `BO ID`
pub fn parse_cdsl_cas(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = vec![
        "Date".to_string(),
        "Transaction Description".to_string(),
        "Security Description".to_string(),
        "ISIN".to_string(),
        "Quantity".to_string(),
        "Rate".to_string(),
        "Amount".to_string(),
        "BO ID".to_string(),
    ];

    let mut txn_rows: Vec<Vec<String>> = Vec::new();
    let mut holdings_rows: Vec<Vec<String>> = Vec::new();
    let mut current_boid = String::new();
    let mut statement_date = String::new();
    let mut in_transaction_section = false;

    let isin_regex = Regex::new(r"\b(IN[A-Z0-9]{10})\b").unwrap();
    let date_regex =
        Regex::new(r"^\b(\d{1,2}[-/][A-Za-z]{3}[-/]\d{2,4}|\d{1,2}[-/]\d{1,2}[-/]\d{2,4})\b")
            .unwrap();
    let boid_regex = Regex::new(r"\b(\d{16})\b").unwrap();
    let mf_code_prefix_regex = Regex::new(r"^[A-Z0-9]{2,8}\s*-\s*").unwrap();

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

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        // Skip lines with Hindi or portfolio summary markers
        if is_summary_or_noise_line(line) {
            i += 1;
            continue;
        }

        let lower = line.to_lowercase();

        // Detect section headers: "HOLDING STATEMENT" vs "STATEMENT OF TRANSACTIONS"
        if lower.contains("statement of transactions") || lower.contains("transaction statement") {
            in_transaction_section = true;
        } else if lower.contains("holding statement")
            || lower.contains("statement of holdings")
            || lower.contains("holding as on")
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
        if (lower.contains("statement as on")
            || lower.contains("as on date")
            || lower.contains("statement date"))
            && statement_date.is_empty()
        {
            let date_match_re =
                Regex::new(r"\b(\d{1,2}[-/][A-Za-z0-9]{2,3}[-/]\d{2,4})\b").unwrap();
            if let Some(m) = date_match_re.find(line) {
                statement_date = m.as_str().to_string();
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
                    // A folio number consists only of digits and optional slash (e.g. "499213345283/0" or "3991001396/0")
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
                        let clean_num = next_line.replace(',', "");
                        if clean_num.parse::<f64>().is_ok() && next_line != "--" {
                            // Reached numbers (CurrentBal / MarketPrice / etc.)
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

                let mut num_cands = Vec::new();
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
                    let clean_num = cand.replace(',', "");
                    if let Ok(num) = clean_num.parse::<f64>() {
                        if cand != "--" {
                            num_cands.push((num, clean_num));
                        }
                    }
                }

                // In CDSL Demat Table:
                // CurrentBal (cand 0), Free Bal (cand 1), MarketPrice (cand 2), Value (cand 3)
                // In CDSL MF Table (after folio):
                // Units (cand 0), NAV (cand 1), Cost (cand 2), Value (cand 3)
                if !num_cands.is_empty() && num_cands[0].0 > 0.0 {
                    found_qty = Some(num_cands[0].1.clone());
                    if num_cands.len() >= 4 {
                        if is_folio_line {
                            // MF Table: Cand 0 = Units, Cand 1 = NAV, Cand 3 = Value
                            found_rate = Some(num_cands[1].1.clone());
                            found_val = Some(num_cands[3].1.clone());
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
                        "HOLDING BALANCE".to_string(),
                        sec_name,
                        isin.clone(),
                        qty,
                        found_rate.unwrap_or_default(),
                        found_val.unwrap_or_default(),
                        current_boid.clone(),
                    ]);
                }
            }
        }

        // 4. Detect Transaction row (starts with Date: e.g. "03-08-2026")
        if let Some(date_match) = date_regex.find(line) {
            let date_str = date_match.as_str().to_string();
            let remainder = line[date_match.end()..].trim();

            if !is_summary_or_noise_line(remainder) {
                let tokens: Vec<&str> = remainder.split_whitespace().collect();
                let mut matched_isin = String::new();
                let mut sec_name = String::new();

                for back in 1..=50 {
                    if i >= back && isin_regex.is_match(lines[i - back]) {
                        if let Some(c) = isin_regex.captures(lines[i - back]) {
                            matched_isin = c.get(1).map_or("", |m| m.as_str()).to_string();
                            let rest = lines[i - back]
                                .replace(&matched_isin, "")
                                .replace("ISIN:", "")
                                .replace("ISIN", "");
                            let s = rest.trim();
                            if s.len() > 3 && !is_summary_or_noise_line(s) {
                                sec_name = clean_security_description(s);
                            }
                            break;
                        }
                    }
                }

                if sec_name.is_empty() {
                    if let Some(mapped_name) = mf_scheme_map.get(&matched_isin) {
                        sec_name = mapped_name.clone();
                    }
                }

                if tokens.len() >= 2 {
                    let mut num_indices = Vec::new();
                    for (idx, t) in tokens.iter().enumerate() {
                        let cleaned = t.replace(',', "").replace('(', "").replace(')', "");
                        if cleaned.parse::<f64>().is_ok() {
                            num_indices.push(idx);
                        }
                    }

                    if !num_indices.is_empty() {
                        let first_num_idx = num_indices[0];
                        let desc = tokens[..first_num_idx].join(" ");
                        let mut qty = String::new();
                        let mut rate = String::new();
                        let mut amount = String::new();

                        if num_indices.len() == 1 {
                            qty = tokens[num_indices[0]].replace(',', "");
                        } else if num_indices.len() == 2 {
                            qty = tokens[num_indices[0]].replace(',', "");
                            rate = tokens[num_indices[1]].replace(',', "");
                        } else if num_indices.len() >= 3 {
                            qty = tokens[num_indices[0]].replace(',', "");
                            rate = tokens[num_indices[1]].replace(',', "");
                            amount = tokens[num_indices[2]].replace(',', "");
                        }

                        let desc_lower = desc.to_lowercase();
                        let is_transaction_word = desc_lower.contains("buy")
                            || desc_lower.contains("sell")
                            || desc_lower.contains("purchase")
                            || desc_lower.contains("sale")
                            || desc_lower.contains("sip")
                            || desc_lower.contains("redemption")
                            || desc_lower.contains("switch")
                            || desc_lower.contains("ipo")
                            || desc_lower.contains("transfer");

                        if is_transaction_word && !desc.is_empty() && !qty.is_empty() {
                            txn_rows.push(vec![
                                date_str.clone(),
                                desc,
                                sec_name.clone(),
                                matched_isin.clone(),
                                qty,
                                rate,
                                amount,
                                current_boid.clone(),
                            ]);
                        }
                    }
                } else if i + 1 < lines.len() {
                    let desc_cand = lines[i + 1];
                    let is_txn_desc = desc_cand.contains("Purchase")
                        || desc_cand.contains("SIP")
                        || desc_cand.contains("BUY")
                        || desc_cand.contains("SALE")
                        || desc_cand.contains("Redemption")
                        || desc_cand.contains("Switch");

                    if is_txn_desc && !is_summary_or_noise_line(desc_cand) {
                        let mut amount = String::new();
                        let mut nav = String::new();
                        let mut units = String::new();

                        let mut num_cands = Vec::new();
                        for offset in 2..=8 {
                            if i + offset < lines.len() {
                                let cand = lines[i + offset].replace(',', "");
                                if cand.parse::<f64>().is_ok() {
                                    num_cands.push(cand);
                                }
                            }
                        }

                        if num_cands.len() >= 4 {
                            amount = num_cands[0].clone();
                            nav = num_cands[1].clone();
                            units = num_cands[3].clone();
                        } else if num_cands.len() == 3 {
                            amount = num_cands[0].clone();
                            nav = num_cands[1].clone();
                            units = num_cands[2].clone();
                        } else if num_cands.len() == 2 {
                            amount = num_cands[0].clone();
                            units = num_cands[1].clone();
                        }

                        if !amount.is_empty() || !units.is_empty() {
                            txn_rows.push(vec![
                                date_str,
                                desc_cand.to_string(),
                                sec_name,
                                matched_isin,
                                units,
                                nav,
                                amount,
                                current_boid.clone(),
                            ]);
                        }
                    }
                }
            }
        }

        i += 1;
    }

    // Default to combined rows (transactions first, then holdings)
    // If only holdings exist, returns holdings rows.
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
        assert_eq!(headers.len(), 8);
        // Hindi portfolio summary line must be excluded!
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "10-May-2024");
        assert_eq!(rows[0][1], "BUY");
        assert_eq!(rows[0][3], "INE002A01018");
        assert_eq!(rows[1][0], "12-May-2024");
        assert_eq!(rows[1][1], "SALE");
    }

    #[test]
    fn test_parse_cdsl_mf_holdings() {
        let sample = r#"
BO ID: 1208160007465501
HOLDING STATEMENT AS ON 31-08-2026

AMC Name : Nippon India Mutual Fund
Folio No : 499213345283/0
Scheme Name : NIPPON INDIA SMALL CAP FUND - DIRECT GROWTH PLAN GROWTH OPTION
ISIN : INF204K01K15

SCAG - NIPPON INDIA SMALL CAP FUND - DIRECT GROWTH PLAN GROWTH OPTION
INF204K01K15
499213345283/0
1042.883
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
        assert_eq!(row[4], "1042.883");
        assert_eq!(row[5], "210.6366");
        assert_eq!(row[6], "219669.33");
        assert_eq!(row[7], "1208160007465501");
    }
}
