//! Which exchange corporate actions an account still needs recorded.

use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, NaiveDate};
use regex::Regex;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;

use crate::accounts::{Account, TrackingMode};
use crate::activities::{
    effective_activity_type, Activity, ActivityStatus, ACTIVITY_SUBTYPE_CONVERSION_IN,
    ACTIVITY_SUBTYPE_CONVERSION_OUT, ACTIVITY_SUBTYPE_SPIN_OFF_IN, ACTIVITY_SUBTYPE_SPIN_OFF_OUT,
    ACTIVITY_TYPE_ADJUSTMENT, ACTIVITY_TYPE_BUY, ACTIVITY_TYPE_SELL, ACTIVITY_TYPE_SPLIT,
    ACTIVITY_TYPE_TRANSFER_IN, ACTIVITY_TYPE_TRANSFER_OUT,
};
use crate::assets::Asset;

/// One corporate action as the exchange announces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeAction {
    pub symbol: String,
    pub isin: Option<String>,
    pub company: Option<String>,
    /// The exchange's wording, e.g. "Bonus 1:1".
    pub subject: String,
    pub ex_date: NaiveDate,
    pub record_date: Option<NaiveDate>,
}

/// What an announcement means for the shares held.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    /// `new` bonus shares for every `held` shares.
    Bonus { new: Decimal, held: Decimal },
    /// Each share becomes `ratio` shares (below 1 for a consolidation).
    Split { ratio: Decimal },
    /// A cash dividend of `per_share` rupees on each share held.
    Dividend { per_share: Decimal },
    /// Changes holdings or cost in a way the announcement doesn't spell out
    /// (demerger, merger, scheme of arrangement, capital reduction).
    NeedsReview,
    /// Doesn't change the shares held or pay cash (AGMs, rights, buybacks).
    Ignored,
}

static BONUS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bbonus\b[^0-9]*(\d+(?:\.\d+)?)\s*:\s*(\d+(?:\.\d+)?)")
        .expect("hard-coded regex is valid")
});
static FACE_VALUE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)from\s*(?:rs\.?|re\.?|inr|₹)?\s*(\d+(?:\.\d+)?)\s*/?-?.*?\bto\s*(?:rs\.?|re\.?|inr|₹)?\s*(\d+(?:\.\d+)?)",
    )
    .expect("hard-coded regex is valid")
});

static DIVIDEND: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)dividend[^/]*?(?:rs\.?|re\.?|inr|₹)\s*(\d+(?:\.\d+)?)")
        .expect("hard-coded regex is valid")
});

/// Reads the exchange's subject line.
pub fn parse_subject(subject: &str) -> ActionKind {
    let lower = subject.to_lowercase();
    if lower.contains("dividend") && !lower.contains("bonus") {
        // "Interim Dividend - Rs 5 Per Share / Special Dividend - Rs 3 Per
        // Share" pays both.
        let per_share: Decimal = DIVIDEND
            .captures_iter(subject)
            .filter_map(|caps| Decimal::from_str(&caps[1]).ok())
            .sum();
        return if per_share > Decimal::ZERO {
            ActionKind::Dividend {
                per_share: per_share.normalize(),
            }
        } else {
            ActionKind::Ignored
        };
    }
    if lower.contains("bonus") {
        if let Some(caps) = BONUS.captures(subject) {
            let new = Decimal::from_str(&caps[1]).ok();
            let held = Decimal::from_str(&caps[2]).ok();
            if let (Some(new), Some(held)) = (new, held) {
                if new > Decimal::ZERO && held > Decimal::ZERO {
                    return ActionKind::Bonus { new, held };
                }
            }
        }
        return ActionKind::NeedsReview;
    }
    let is_split = ["split", "sub-division", "sub division", "subdivision"]
        .iter()
        .any(|w| lower.contains(w));
    let is_consolidation = lower.contains("consolidation");
    if is_split || is_consolidation {
        if let Some(caps) = FACE_VALUE.captures(subject) {
            let from = Decimal::from_str(&caps[1]).ok();
            let to = Decimal::from_str(&caps[2]).ok();
            if let (Some(from), Some(to)) = (from, to) {
                if from > Decimal::ZERO && to > Decimal::ZERO && from != to {
                    return ActionKind::Split {
                        ratio: (from / to).normalize(),
                    };
                }
            }
        }
        return ActionKind::NeedsReview;
    }
    if [
        "demerger",
        "de-merger",
        "amalgamation",
        "merger",
        "scheme of arrangement",
        "capital reduction",
        "reduction of capital",
    ]
    .iter()
    .any(|w| lower.contains(w))
    {
        return ActionKind::NeedsReview;
    }
    ActionKind::Ignored
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProposalKind {
    Bonus,
    Split,
    Dividend,
    Review,
}

/// One action for one account's holding of one share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorporateActionProposal {
    pub kind: ProposalKind,
    pub account_id: String,
    pub account_name: String,
    pub asset_id: String,
    pub symbol: String,
    pub name: Option<String>,
    pub subject: String,
    pub ex_date: NaiveDate,
    pub record_date: Option<NaiveDate>,
    /// The date to record the activity on: the record date, else the ex-date.
    pub activity_date: NaiveDate,
    /// Shares held at the end of the day before the ex-date.
    pub held_quantity: Decimal,
    /// Bonus shares to add (whole shares; fractions are paid out in cash).
    pub bonus_quantity: Option<Decimal>,
    /// Split ratio: shares after for each share before.
    pub split_ratio: Option<Decimal>,
    /// Dividend per share, in rupees.
    pub dividend_per_share: Option<Decimal>,
    /// Dividend before TDS: shares held × the rate.
    pub dividend_amount: Option<Decimal>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorporateActionProposals {
    pub proposals: Vec<CorporateActionProposal>,
    /// Actions already in the account's activities.
    pub already_recorded: usize,
    /// Symbols checked.
    pub symbols_checked: Vec<String>,
    /// Symbols the exchange returned nothing for, with the reason.
    pub failed_symbols: Vec<(String, String)>,
}

/// Days either side of the ex-date in which an existing activity counts as
/// the action already being recorded (allotment can trail the record date by
/// a few days, and a hand-entered date may differ).
const RECORDED_WINDOW_DAYS: i64 = 15;

/// A dividend is paid up to 30 days after its record date, and a dividend
/// recorded from a bank or broker statement carries the payment date.
const DIVIDEND_WINDOW_BEFORE_DAYS: i64 = 5;
const DIVIDEND_WINDOW_AFTER_DAYS: i64 = 45;

fn day(activity: &Activity) -> NaiveDate {
    activity.activity_date.date_naive()
}

fn subtype(activity: &Activity) -> &str {
    activity.subtype.as_deref().unwrap_or("")
}

/// Quantity held at the end of `before` (exclusive) from the activities of
/// one account and asset, in date order.
fn held_before(activities: &[&Activity], before: NaiveDate) -> Decimal {
    let mut held = Decimal::ZERO;
    for activity in activities.iter().filter(|a| day(a) < before) {
        let qty = activity.quantity.unwrap_or_default().abs();
        match effective_activity_type(
            &activity.activity_type,
            activity.activity_type_override.as_deref(),
        ) {
            ACTIVITY_TYPE_BUY | ACTIVITY_TYPE_TRANSFER_IN => held += qty,
            ACTIVITY_TYPE_SELL | ACTIVITY_TYPE_TRANSFER_OUT => held -= qty,
            ACTIVITY_TYPE_ADJUSTMENT => match subtype(activity) {
                ACTIVITY_SUBTYPE_CONVERSION_IN | ACTIVITY_SUBTYPE_SPIN_OFF_IN => held += qty,
                ACTIVITY_SUBTYPE_CONVERSION_OUT | ACTIVITY_SUBTYPE_SPIN_OFF_OUT => held -= qty,
                _ => {}
            },
            ACTIVITY_TYPE_SPLIT => {
                if let Some(ratio) = activity.amount.filter(|r| *r > Decimal::ZERO) {
                    held *= ratio;
                }
            }
            _ => {}
        }
    }
    held.max(Decimal::ZERO)
}

fn is_recorded(kind: &ActionKind, activities: &[&Activity], ex_date: NaiveDate) -> bool {
    let window = Duration::days(RECORDED_WINDOW_DAYS);
    let near = |a: &&&Activity| (day(a) - ex_date).abs() <= window;
    let kind_of = |a: &Activity| {
        effective_activity_type(&a.activity_type, a.activity_type_override.as_deref()).to_string()
    };
    if matches!(kind, ActionKind::Dividend { .. }) {
        return activities.iter().any(|a| {
            let offset = (day(a) - ex_date).num_days();
            kind_of(a) == crate::activities::ACTIVITY_TYPE_DIVIDEND
                && (-DIVIDEND_WINDOW_BEFORE_DAYS..=DIVIDEND_WINDOW_AFTER_DAYS).contains(&offset)
        });
    }
    activities.iter().filter(near).any(|a| {
        let ty = kind_of(a);
        let converted = ty == ACTIVITY_TYPE_ADJUSTMENT
            && matches!(
                subtype(a),
                ACTIVITY_SUBTYPE_CONVERSION_OUT | ACTIVITY_SUBTYPE_SPIN_OFF_OUT
            );
        match kind {
            ActionKind::Bonus { .. } => {
                ty == ACTIVITY_TYPE_BUY
                    && a.unit_price.unwrap_or_default().is_zero()
                    && a.amount.unwrap_or_default().is_zero()
            }
            // A split the depository showed as an ISIN change is recorded as a
            // conversion out of the old ISIN.
            ActionKind::Split { .. } => ty == ACTIVITY_TYPE_SPLIT || converted,
            ActionKind::NeedsReview => converted,
            ActionKind::Dividend { .. } => unreachable!("handled above"),
            ActionKind::Ignored => true,
        }
    })
}

/// The NSE symbol of an asset, when it is an NSE-listed (or Indian, rupee
/// quoted) share.
pub(crate) fn nse_symbol(asset: &Asset) -> Option<String> {
    let symbol = asset.instrument_symbol.as_deref()?.trim();
    if symbol.is_empty() {
        return None;
    }
    let mic = asset.instrument_exchange_mic.as_deref().unwrap_or("");
    let indian = mic.eq_ignore_ascii_case("XNSE")
        || (mic.is_empty() && asset.quote_ccy.eq_ignore_ascii_case("INR"));
    indian.then(|| {
        symbol
            .trim_end_matches(".NS")
            .trim_end_matches(".NSE")
            .to_ascii_uppercase()
    })
}

/// The NSE symbols to look up: each share a transactions account holds today
/// and, when dividends are wanted from `dividends_since`, each share it held
/// at any time from that day.
pub fn symbols_to_check(
    accounts: &[Account],
    assets: &[Asset],
    activities: &[Activity],
    today: NaiveDate,
    dividends_since: Option<NaiveDate>,
) -> Vec<String> {
    let assets_by_id: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut symbols: Vec<String> = holdings(accounts, activities)
        .into_iter()
        .filter(|(_, list)| {
            held_before(list, today + Duration::days(1)) > Decimal::ZERO
                || dividends_since.is_some_and(|since| {
                    held_before(list, since) > Decimal::ZERO || list.iter().any(|a| day(a) >= since)
                })
        })
        .filter_map(|((_, asset_id), _)| assets_by_id.get(asset_id).copied())
        .filter(|asset| asset.is_equity_like())
        .filter_map(nse_symbol)
        .collect();
    symbols.sort();
    symbols.dedup();
    symbols
}

/// Posted activities of each transactions account's holdings, in date order.
fn holdings<'a>(
    accounts: &'a [Account],
    activities: &'a [Activity],
) -> BTreeMap<(&'a str, &'a str), Vec<&'a Activity>> {
    let tracked: std::collections::HashSet<&str> = accounts
        .iter()
        .filter(|a| a.tracking_mode == TrackingMode::Transactions && !a.is_archived)
        .map(|a| a.id.as_str())
        .collect();
    let mut by_holding: BTreeMap<(&str, &str), Vec<&Activity>> = BTreeMap::new();
    for activity in activities {
        if activity.status != ActivityStatus::Posted
            || !tracked.contains(activity.account_id.as_str())
        {
            continue;
        }
        if let Some(asset_id) = activity.asset_id.as_deref() {
            by_holding
                .entry((activity.account_id.as_str(), asset_id))
                .or_default()
                .push(activity);
        }
    }
    for list in by_holding.values_mut() {
        list.sort_by_key(|a| a.activity_date);
    }
    by_holding
}

/// The actions each transactions account still needs: for every share it held
/// at an ex-date on or before `today`, a bonus or split not yet among its
/// activities, or an action that needs the user's input; and, for ex-dates
/// from `dividends_since`, each dividend not yet recorded (before TDS).
pub fn propose_corporate_actions(
    accounts: &[Account],
    assets: &[Asset],
    activities: &[Activity],
    actions: &HashMap<String, Vec<ExchangeAction>>,
    today: NaiveDate,
    dividends_since: Option<NaiveDate>,
) -> (Vec<CorporateActionProposal>, usize) {
    let assets_by_id: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_holding = holdings(accounts, activities);

    let mut proposals = Vec::new();
    let mut recorded = 0;
    for account in accounts
        .iter()
        .filter(|a| a.tracking_mode == TrackingMode::Transactions && !a.is_archived)
    {
        for ((account_id, asset_id), list) in &by_holding {
            if *account_id != account.id {
                continue;
            }
            let Some(asset) = assets_by_id.get(asset_id) else {
                continue;
            };
            if !asset.is_equity_like() {
                continue;
            }
            let Some(symbol) = nse_symbol(asset) else {
                continue;
            };
            let Some(announced) = actions.get(&symbol) else {
                continue;
            };
            for action in announced.iter().filter(|a| a.ex_date <= today) {
                let kind = parse_subject(&action.subject);
                if kind == ActionKind::Ignored {
                    continue;
                }
                if matches!(kind, ActionKind::Dividend { .. })
                    && dividends_since.is_none_or(|since| action.ex_date < since)
                {
                    continue;
                }
                let held = held_before(list, action.ex_date);
                if held <= Decimal::ZERO {
                    continue;
                }
                if is_recorded(&kind, list, action.ex_date) {
                    recorded += 1;
                    continue;
                }
                let mut dividend = None;
                let (proposal_kind, bonus_quantity, split_ratio) = match kind {
                    ActionKind::Bonus { new, held: per } => {
                        let shares = (held * new / per).floor();
                        if shares <= Decimal::ZERO {
                            continue;
                        }
                        (ProposalKind::Bonus, Some(shares), None)
                    }
                    ActionKind::Split { ratio } => (ProposalKind::Split, None, Some(ratio)),
                    ActionKind::Dividend { per_share } => {
                        dividend = Some((per_share, (held * per_share).round_dp(2)));
                        (ProposalKind::Dividend, None, None)
                    }
                    _ => (ProposalKind::Review, None, None),
                };
                proposals.push(CorporateActionProposal {
                    kind: proposal_kind,
                    account_id: account.id.clone(),
                    account_name: account.name.clone(),
                    asset_id: asset.id.clone(),
                    symbol: symbol.clone(),
                    name: asset.name.clone().or_else(|| action.company.clone()),
                    subject: action.subject.clone(),
                    ex_date: action.ex_date,
                    record_date: action.record_date,
                    activity_date: action.record_date.unwrap_or(action.ex_date),
                    held_quantity: held.normalize(),
                    bonus_quantity,
                    split_ratio,
                    dividend_per_share: dividend.map(|(rate, _)| rate),
                    dividend_amount: dividend.map(|(_, amount)| amount.normalize()),
                });
            }
        }
    }
    proposals.sort_by(|a, b| {
        (a.ex_date, &a.account_name, &a.symbol).cmp(&(b.ex_date, &b.account_name, &b.symbol))
    });
    (proposals, recorded)
}
