use crate::errors::MarketDataError;
use crate::models::SearchResult;

#[derive(Debug)]
pub(super) struct NavScheme {
    pub code: String,
    pub name: String,
    pub isins: Vec<String>,
    pub is_etf: bool,
}

pub(super) fn is_etf_category(category: &str) -> bool {
    category.starts_with("Exchange Traded Funds (ETFs)")
        || matches!(
            category,
            "Other Scheme - Gold ETF" | "Other Scheme - Other ETFs"
        )
}

pub(super) fn parse(text: &str) -> Result<Vec<NavScheme>, MarketDataError> {
    let invalid_feed = || MarketDataError::ProviderError {
        provider: super::PROVIDER_ID.to_string(),
        message: "Invalid AMFI NAV catalogue".to_string(),
    };
    let mut columns: Option<Vec<&str>> = None;
    let mut schemes = Vec::new();
    let mut is_etf = false;
    for line in text
        .lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}'))
    {
        if line.starts_with("Scheme Code;") {
            let header: Vec<_> = line.split(';').map(str::trim).collect();
            for required in [
                "Scheme Code",
                "ISIN Div Payout/ ISIN Growth",
                "ISIN Div Reinvestment",
                "Scheme Name",
                "Net Asset Value",
                "Date",
            ] {
                if !header.contains(&required) {
                    return Err(invalid_feed());
                }
            }
            columns = Some(header);
            continue;
        }
        if line.starts_with("Open Ended Schemes")
            || line.starts_with("Close Ended Schemes")
            || line.starts_with("Interval Fund Schemes")
        {
            let category = line
                .split_once('(')
                .map(|(_, category)| category.strip_suffix(')').unwrap_or(category))
                .unwrap_or_default();
            is_etf = is_etf_category(category);
            continue;
        }
        if !line
            .split(';')
            .next()
            .is_some_and(|code| !code.is_empty() && code.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let header = columns.as_ref().ok_or_else(invalid_feed)?;
        let values: Vec<_> = line.split(';').map(str::trim).collect();
        if values.len() != header.len() {
            return Err(invalid_feed());
        }
        let value = |key: &str| {
            header
                .iter()
                .position(|column| *column == key)
                .map(|index| values[index])
                .unwrap_or_default()
        };
        let code = value("Scheme Code");
        let name = value("Scheme Name");
        if code.parse::<u64>().ok().is_none_or(|code| code == 0) || name.is_empty() {
            return Err(invalid_feed());
        }
        let name = [name, value("Plan"), value("Option")]
            .into_iter()
            .filter(|part| !part.is_empty() && *part != "-")
            .collect::<Vec<_>>()
            .join(" - ");
        let mut isins = Vec::new();
        for key in ["ISIN Div Payout/ ISIN Growth", "ISIN Div Reinvestment"] {
            let isin = value(key).to_ascii_uppercase();
            if isin.len() == 12
                && isin.starts_with("IN")
                && isin.chars().all(|c| c.is_ascii_alphanumeric())
                && !isins.contains(&isin)
            {
                isins.push(isin);
            }
        }
        schemes.push(NavScheme {
            code: code.to_string(),
            name,
            isins,
            is_etf,
        });
    }
    if schemes.is_empty() {
        return Err(invalid_feed());
    }
    Ok(schemes)
}

pub(super) fn lookup(schemes: &[NavScheme], isin: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut codes = std::collections::HashSet::new();
    for scheme in schemes {
        if scheme.isins.iter().any(|candidate| candidate == isin) && codes.insert(&scheme.code) {
            let mut result = SearchResult::new(
                &scheme.code,
                &scheme.name,
                "AMFI",
                if scheme.is_etf { "ETF" } else { "MUTUALFUND" },
            )
            .with_currency("INR")
            .with_data_source(super::PROVIDER_ID)
            .with_score(25000.0);
            result.isin = Some(isin.to_string());
            results.push(result);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Scheme Code;ISIN Div Payout/ ISIN Growth;ISIN Div Reinvestment;Scheme Name;Plan;Option;Net Asset Value;Date";

    #[test]
    fn parses_split_plan_and_option_with_both_isins() {
        let text = format!("{HEADER}\nOpen Ended Schemes(Equity Scheme - Small Cap Fund)\n100001;INF000000001;INF000000002;Example Fund;Direct Plan;IDCW Option;10.5;01-Oct-2026\n");
        let schemes = parse(&text).unwrap();
        assert_eq!(schemes.len(), 1);
        assert_eq!(schemes[0].code, "100001");
        assert_eq!(schemes[0].name, "Example Fund - Direct Plan - IDCW Option");
        assert_eq!(schemes[0].isins, ["INF000000001", "INF000000002"]);
        assert!(!schemes[0].is_etf);
    }

    #[test]
    fn parses_legacy_feed_and_etf_category() {
        let text = "Scheme Code;ISIN Div Payout/ ISIN Growth;ISIN Div Reinvestment;Scheme Name;Net Asset Value;Date\nOpen Ended Schemes(Exchange Traded Funds (ETFs) - Gold ETF)\n100001;INF000000001;-;Example Gold;10.5;01-Oct-2026\nOpen Ended Schemes(Equity Scheme - Small Cap Fund)\nExample Mutual Fund\n100002;-;INF000000002;Example Small Cap Direct Growth;20;01-Oct-2026\n";
        let schemes = parse(text).unwrap();
        assert_eq!(schemes.len(), 2);
        assert!(schemes[0].is_etf);
        assert!(!schemes[1].is_etf);
        assert_eq!(schemes[1].name, "Example Small Cap Direct Growth");
        assert_eq!(schemes[1].isins, ["INF000000002"]);
    }

    #[test]
    fn exact_lookup_exposes_conflicting_codes_but_deduplicates_same_code() {
        let text = format!("{HEADER}\n100001;INF000000001;INF000000002;Example Fund;Direct Plan;Growth Option;10;01-Oct-2026\n100001;INF000000001;INF000000002;Example Fund;Direct Plan;Growth Option;10;01-Oct-2026\n100002;INF000000002;-;Other Fund;Regular Plan;Growth Option;20;01-Oct-2026\n");
        let schemes = parse(&text).unwrap();
        assert_eq!(lookup(&schemes, "INF000000001").len(), 1);
        assert_eq!(lookup(&schemes, "INF000000001")[0].symbol, "100001");
        let conflicting = lookup(&schemes, "INF000000002");
        assert_eq!(conflicting.len(), 2);
        assert_eq!(conflicting[0].symbol, "100001");
        assert_eq!(conflicting[1].symbol, "100002");
        assert!(lookup(&schemes, "INF000000003").is_empty());
    }

    #[test]
    fn rejects_invalid_feed_instead_of_caching_an_empty_catalogue() {
        assert!(parse("<html>Service unavailable</html>").is_err());
        assert!(parse(&format!("{HEADER}\n100001;broken row\n")).is_err());
    }

    #[test]
    fn classifies_legacy_etf_categories_without_guessing_from_names() {
        for (category, expected) in [
            ("Other Scheme - Gold ETF", "ETF"),
            ("Other Scheme - Other ETFs", "ETF"),
            ("Other Scheme - FoF Domestic", "MUTUALFUND"),
            ("Other Scheme - Index Funds", "MUTUALFUND"),
        ] {
            let text = format!("{HEADER}\nOpen Ended Schemes({category})\n100001;INF000000001;-;Example ETF Fund of Funds;Direct Plan;Growth Option;10;01-Oct-2026\n");
            let schemes = parse(&text).unwrap();
            assert_eq!(lookup(&schemes, "INF000000001")[0].asset_type, expected);
        }
    }
}
