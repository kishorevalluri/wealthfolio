//! Dividends and interest for one Indian financial year, as Schedule OS of the
//! ITR asks for them: gross amounts, tax deducted at source, and dividends by
//! the advance-tax periods (for the Section 234C interest calculation).

use std::collections::BTreeMap;

use chrono::{Datelike, FixedOffset, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::activities::{
    Activity, ACTIVITY_TYPE_DIVIDEND, ACTIVITY_TYPE_INTEREST, ACTIVITY_TYPE_TAX,
};

/// Dividends received from one asset in the year.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianDividendByAsset {
    pub asset_id: String,
    pub gross: Decimal,
    pub tds: Decimal,
}

/// Income from other sources found in the activities of one financial year.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianOtherSourcesSummary {
    /// Dividends before TDS (cash received plus tax withheld)
    pub dividends_gross: Decimal,
    pub dividends_tds: Decimal,
    /// Interest before TDS
    pub interest_gross: Decimal,
    pub interest_tds: Decimal,
    /// Gross dividends by the periods Schedule OS asks for: up to 15 Jun,
    /// 16 Jun-15 Sep, 16 Sep-15 Dec, 16 Dec-15 Mar, 16 Mar-31 Mar.
    pub dividends_by_period: [Decimal; 5],
    pub dividends_by_asset: Vec<IndianDividendByAsset>,
}

/// Activities are stored in UTC; the periods are Indian dates.
fn indian_date(activity: &Activity) -> NaiveDate {
    let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST offset is in range");
    activity.activity_date.with_timezone(&ist).date_naive()
}

/// Which Schedule OS period a date of the year falls in.
fn period(date: NaiveDate) -> usize {
    let (m, d) = (date.month(), date.day());
    match (m, d) {
        (4 | 5, _) | (6, 1..=15) => 0,
        (6, _) | (7 | 8, _) | (9, 1..=15) => 1,
        (9, _) | (10 | 11, _) | (12, 1..=15) => 2,
        (12, _) | (1 | 2, _) | (3, 1..=15) => 3,
        _ => 4,
    }
}

fn abs(value: Option<Decimal>) -> Decimal {
    value.unwrap_or_default().abs()
}

/// Sums the dividends and interest of financial year `fy_start_year`.
///
/// A dividend or interest row's `amount` is the cash received, so its `tax`
/// (TDS) is added back for the gross. A separate TAX row with the
/// `WITHHOLDING` subtype counts as TDS too: on a dividend when it names an
/// asset, on interest otherwise.
pub fn summarize_other_sources(
    activities: &[Activity],
    fy_start_year: i32,
) -> IndianOtherSourcesSummary {
    let mut summary = IndianOtherSourcesSummary::default();
    let (Some(start), Some(end)) = (
        NaiveDate::from_ymd_opt(fy_start_year, 4, 1),
        NaiveDate::from_ymd_opt(fy_start_year + 1, 3, 31),
    ) else {
        return summary;
    };
    let mut by_asset: BTreeMap<String, IndianDividendByAsset> = BTreeMap::new();

    for activity in activities {
        if !activity.is_posted() {
            continue;
        }
        let date = indian_date(activity);
        if date < start || date > end {
            continue;
        }
        let amount = abs(activity.amount);
        let tax = abs(activity.tax);
        let asset_id = activity.asset_id.clone().unwrap_or_default();
        match activity.effective_type() {
            ACTIVITY_TYPE_DIVIDEND => {
                let gross = amount + tax;
                summary.dividends_gross += gross;
                summary.dividends_tds += tax;
                summary.dividends_by_period[period(date)] += gross;
                let row = by_asset.entry(asset_id.clone()).or_default();
                row.asset_id = asset_id;
                row.gross += gross;
                row.tds += tax;
            }
            ACTIVITY_TYPE_INTEREST => {
                summary.interest_gross += amount + tax;
                summary.interest_tds += tax;
            }
            ACTIVITY_TYPE_TAX
                if activity
                    .subtype
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case("WITHHOLDING")) =>
            {
                if asset_id.is_empty() {
                    summary.interest_tds += amount;
                } else {
                    summary.dividends_tds += amount;
                    let row = by_asset.entry(asset_id.clone()).or_default();
                    row.asset_id = asset_id;
                    row.tds += amount;
                }
            }
            _ => {}
        }
    }
    summary.dividends_by_asset = by_asset.into_values().collect();
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn activity(kind: &str, date: &str, amount: Decimal, tax: Option<Decimal>) -> Activity {
        let mut a: Activity = serde_json::from_value(serde_json::json!({
            "id": "a",
            "accountId": "acc",
            "assetId": "INFY",
            "activityType": kind,
            "status": "POSTED",
            "activityDate": format!("{date}T00:00:00Z"),
            "currency": "INR",
            "isUserModified": false,
            "needsReview": false,
            "createdAt": "2025-01-01T00:00:00Z",
            "updatedAt": "2025-01-01T00:00:00Z",
        }))
        .expect("test activity");
        a.amount = Some(amount);
        a.tax = tax;
        a
    }

    #[test]
    fn periods_follow_the_advance_tax_dates() {
        let d = |m, day| NaiveDate::from_ymd_opt(2025, m, day).unwrap();
        assert_eq!(period(d(6, 15)), 0);
        assert_eq!(period(d(6, 16)), 1);
        assert_eq!(period(d(9, 16)), 2);
        assert_eq!(period(d(12, 16)), 3);
        assert_eq!(period(NaiveDate::from_ymd_opt(2026, 3, 15).unwrap()), 3);
        assert_eq!(period(NaiveDate::from_ymd_opt(2026, 3, 16).unwrap()), 4);
    }

    #[test]
    fn dividends_are_grossed_up_by_tds_within_the_year() {
        let rows = [
            // 900 received after 100 TDS.
            activity("DIVIDEND", "2025-07-01", dec!(900), Some(dec!(100))),
            // 18:45 UTC on 31 Mar is 1 Apr in India: next year.
            {
                let mut a = activity("DIVIDEND", "2026-03-31", dec!(50), None);
                a.activity_date += chrono::Duration::minutes(18 * 60 + 45);
                a
            },
            activity("INTEREST", "2025-04-02", dec!(450), Some(dec!(50))),
            activity("DIVIDEND", "2025-03-31", dec!(70), None),
        ];
        let s = summarize_other_sources(&rows, 2025);
        assert_eq!(s.dividends_gross, dec!(1000));
        assert_eq!(s.dividends_tds, dec!(100));
        assert_eq!(s.dividends_by_period[1], dec!(1000));
        assert_eq!(s.interest_gross, dec!(500));
        assert_eq!(s.interest_tds, dec!(50));
        assert_eq!(s.dividends_by_asset.len(), 1);
    }
}
