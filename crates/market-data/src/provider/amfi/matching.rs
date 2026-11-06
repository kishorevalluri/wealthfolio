fn compact(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn variants(name: &str) -> [Option<u8>; 3] {
    let name = name.rfind("fund").map_or(name, |index| &name[index + 4..]);
    [
        if name.contains("direct") {
            Some(1)
        } else if name.contains("regular") {
            Some(2)
        } else {
            None
        },
        if name.contains("growth") {
            Some(1)
        } else if name.contains("idcw") || name.contains("dividend") {
            Some(2)
        } else {
            None
        },
        if name.contains("reinvest") {
            Some(1)
        } else if name.contains("payout") {
            Some(2)
        } else {
            None
        },
    ]
}

pub fn has_matching_variants(query: &str, name: &str) -> bool {
    let query = variants(&compact(query));
    let name = variants(&compact(name));
    query[0].is_some() && query[1].is_some() && query == name
}

pub fn name_similarity(query: &str, name: &str) -> f64 {
    let query = compact(query);
    let name = compact(name);
    if variants(&query)
        .iter()
        .zip(variants(&name))
        .any(|(a, b)| a.is_some() && b.is_some() && *a != b)
    {
        return 0.0;
    }
    let base = |name: String| {
        let split = name.rfind("fund").map_or(0, |index| index + 4);
        let mut suffix = name[split..].to_string();
        for word in [
            "reinvestment",
            "reinvest",
            "dividend",
            "regular",
            "direct",
            "growth",
            "payout",
            "option",
            "idcw",
            "plan",
        ] {
            suffix = suffix.replace(word, "");
        }
        format!("{}{}", &name[..split], suffix)
    };
    let query = base(query);
    let name = base(name);
    let numbers = |name: &str| -> Vec<String> {
        name.split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    };
    if numbers(&query) != numbers(&name) {
        return 0.0;
    }
    if query.len() < 8 || name.len() < 8 {
        return 0.0;
    }
    if query == name {
        return 1.0;
    }
    let max_edits = query.len().max(name.len()) / 10;
    if query.len().abs_diff(name.len()) > max_edits {
        return 0.0;
    }
    let mut row: Vec<usize> = (0..=name.len()).collect();
    for (i, a) in query.bytes().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in name.bytes().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (row[j] + 1)
                .min(previous + 1)
                .min(diagonal + usize::from(a != b));
            diagonal = previous;
        }
        // Stop distant catalogue candidates early without requiring an exact AMC prefix.
        if row.iter().copied().min().unwrap_or_default() > max_edits {
            return 0.0;
        }
    }
    1.0 - row[name.len()] as f64 / query.len().max(name.len()) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_missing_spaces_and_redundant_suffixes() {
        assert_eq!(
            name_similarity(
                "NIPPONINDIASMALLCAPFUND-DIRECTGROWTHPLANGROWTHOPTION",
                "Nippon India Small Cap Fund - Direct Plan - Growth"
            ),
            1.0
        );
    }

    #[test]
    fn tolerates_small_extraction_errors() {
        assert!(
            name_similarity(
                "Nippon India Smal Cap Fund Direct Growth",
                "Nippon India Small Cap Fund Direct Growth"
            ) > 0.9
        );
    }

    #[test]
    fn rejects_conflicting_variants() {
        for name in [
            "Nippon India Small Cap Fund Regular Growth",
            "Nippon India Small Cap Fund Direct IDCW",
        ] {
            assert_eq!(
                name_similarity("Nippon India Small Cap Fund Direct Growth", name),
                0.0
            );
        }
        assert_eq!(
            name_similarity(
                "HDFC Equity Fund Direct IDCW Payout",
                "HDFC Equity Fund Direct IDCW Reinvestment"
            ),
            0.0
        );
    }

    #[test]
    fn automatic_matching_requires_complete_variants() {
        assert!(!has_matching_variants(
            "HDFC Equity Fund",
            "HDFC Equity Fund Direct Growth"
        ));
        assert!(!has_matching_variants(
            "HDFC Equity Fund Direct IDCW",
            "HDFC Equity Fund Direct IDCW Payout"
        ));
        assert!(has_matching_variants(
            "HDFC Equity Fund Direct Dividend Reinvestment",
            "HDFC Equity Fund Direct IDCW Reinvestment"
        ));
    }

    #[test]
    fn rejects_unrelated_or_short_names() {
        assert_eq!(
            name_similarity(
                "Nippon India Small Cap Fund Direct Growth",
                "HDFC Small Cap Fund Direct Growth"
            ),
            0.0
        );
        assert_eq!(name_similarity("fund", "fund"), 0.0);
    }

    #[test]
    fn preserves_growth_when_it_is_part_of_the_fund_name() {
        assert!(name_similarity("HDFC Growth Fund Direct Growth", "HDFC Fund Direct Growth") < 0.9);
        assert!(!has_matching_variants(
            "HDFC Growth Fund Direct",
            "HDFC Growth Fund Direct Growth"
        ));
    }

    #[test]
    fn rejects_different_numbered_series() {
        assert_eq!(
            name_similarity(
                "HDFC Fixed Maturity Plan Series 100 Fund Direct Growth",
                "HDFC Fixed Maturity Plan Series 101 Fund Direct Growth"
            ),
            0.0
        );
    }

    #[test]
    fn tolerates_a_fund_house_extraction_typo() {
        assert!(
            name_similarity(
                "Nipon India Small Cap Fund Direct Growth",
                "Nippon India Small Cap Fund Direct Growth"
            ) >= 0.9
        );
    }
}
