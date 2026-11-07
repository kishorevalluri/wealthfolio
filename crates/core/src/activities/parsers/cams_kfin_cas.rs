//! CAMS / KFintech Mutual Fund Consolidated Account Statement (CAS) parser.

use regex::Regex;

/// Parse CAMS / KFintech Mutual Fund Consolidated Account Statement.
///
/// Output columns matching `system_cams_cas`:
/// - `Date`
/// - `Transaction Description`
/// - `Scheme Name`
/// - `ISIN`
/// - `Units`
/// - `NAV`
/// - `Amount`
/// - `Folio No`
pub fn parse_cams_kfin_cas(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = vec![
        "Date".to_string(),
        "Transaction Description".to_string(),
        "Scheme Name".to_string(),
        "ISIN".to_string(),
        "Units".to_string(),
        "NAV".to_string(),
        "Amount".to_string(),
        "Folio No".to_string(),
    ];

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_folio = String::new();
    let mut current_scheme = String::new();
    let mut current_isin = String::new();

    let isin_regex = Regex::new(r"\b(INF[A-Z0-9]{9})\b").expect("hard-coded regex is valid");
    let folio_regex = Regex::new(r"(?i)Folio(?:\s*No\.?)?[:\s]+([0-9A-Za-z/_-]+)")
        .expect("hard-coded regex is valid");
    let date_regex =
        Regex::new(r"^\b(\d{1,2}[-/][A-Za-z]{3}[-/]\d{2,4}|\d{1,2}[-/]\d{1,2}[-/]\d{2,4})\b")
            .expect("hard-coded regex is valid");

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 1. Detect Folio Number
        if let Some(caps) = folio_regex.captures(trimmed) {
            current_folio = caps.get(1).map_or("", |m| m.as_str()).to_string();
        }

        // 2. Detect ISIN
        if let Some(caps) = isin_regex.captures(trimmed) {
            current_isin = caps.get(1).map_or("", |m| m.as_str()).to_string();
        }

        // 3. Detect Scheme Name
        let lower = trimmed.to_lowercase();
        if (lower.contains("fund")
            || lower.contains("growth")
            || lower.contains("dividend")
            || lower.contains("direct plan")
            || lower.contains("regular plan"))
            && !lower.starts_with("date")
            && !lower.contains("consolidated")
            && !lower.contains("statement")
            && !date_regex.is_match(trimmed)
            && trimmed.len() > 10
        {
            let mut s = trimmed;
            if let Some(idx) = s.to_lowercase().find("scheme") {
                if let Some(colon) = s[idx..].find(':') {
                    s = s[idx + colon + 1..].trim();
                }
            }
            if !s.is_empty() && !s.contains("Registrar") {
                current_scheme = s.to_string();
            }
        }

        // 4. Detect Transaction Row (starts with a Date)
        if let Some(date_match) = date_regex.find(trimmed) {
            let date_str = date_match.as_str().to_string();
            let remainder = trimmed[date_match.end()..].trim();

            let tokens: Vec<&str> = remainder.split_whitespace().collect();
            if tokens.len() >= 2 {
                let mut num_indices = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    let cleaned = t.replace([',', '(', ')'], "");
                    if cleaned.parse::<f64>().is_ok() {
                        num_indices.push(i);
                    }
                }

                if num_indices.len() >= 2 {
                    let first_num_idx = num_indices[0];
                    let desc = tokens[..first_num_idx].join(" ");

                    let mut amount = "".to_string();
                    let mut units = "".to_string();
                    let mut nav = "".to_string();

                    if num_indices.len() == 2 {
                        amount = tokens[num_indices[0]].replace(',', "");
                        units = tokens[num_indices[1]].replace(',', "");
                    } else if num_indices.len() >= 3 {
                        amount = tokens[num_indices[0]].replace(',', "");
                        units = tokens[num_indices[1]].replace(',', "");
                        nav = tokens[num_indices[2]].replace(',', "");
                    }

                    if !desc.is_empty() && (!amount.is_empty() || !units.is_empty()) {
                        rows.push(vec![
                            date_str,
                            desc,
                            current_scheme.clone(),
                            current_isin.clone(),
                            units,
                            nav,
                            amount,
                            current_folio.clone(),
                        ]);
                    }
                }
            }
        }
    }

    (headers, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cams_kfin_sample() {
        let sample = r#"
Folio No: 987654321
Scheme : HDFC Flexi Cap Fund - Direct Plan - Growth Option
ISIN: INF179K01BE2
01-Jan-2024 Purchase - SIP 5,000.00 35.123 142.35 1500.25
15-Jan-2024 Dividend Payout 250.00 0.00 145.10 1500.25
"#;
        let (headers, rows) = parse_cams_kfin_cas(sample);
        assert_eq!(headers.len(), 8);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "01-Jan-2024");
        assert_eq!(rows[0][1], "Purchase - SIP");
        assert_eq!(
            rows[0][2],
            "HDFC Flexi Cap Fund - Direct Plan - Growth Option"
        );
        assert_eq!(rows[0][3], "INF179K01BE2");
        assert_eq!(rows[0][4], "35.123");
        assert_eq!(rows[0][5], "142.35");
        assert_eq!(rows[0][6], "5000.00");
        assert_eq!(rows[0][7], "987654321");
    }
}
