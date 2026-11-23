//! Corporate actions published by the exchange (NSE) for the shares an account
//! holds: bonuses and splits that fell while the shares were held, proposed as
//! activities for the user to add, and other actions (demergers, mergers) that
//! need the user's input, flagged for review.
//!
//! [`propose_corporate_actions`] is pure: it reads accounts, assets,
//! activities and the exchange's announcements. [`NseClient`] fetches the
//! announcements from NSE's website.

mod nse;
mod proposals;

pub use nse::{NseClient, NSE_SOURCE};

pub use proposals::{
    parse_subject, propose_corporate_actions, symbols_to_check, ActionKind,
    CorporateActionProposal, CorporateActionProposals, ExchangeAction, ProposalKind,
};

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::{accounts::Account, activities::Activity, assets::Asset};

/// Asks NSE for the actions of every share the accounts hold and proposes
/// the ones still missing. A symbol NSE refuses is listed in
/// `failed_symbols`; after three refusals in a row with nothing fetched, the
/// rest are skipped, since NSE is then refusing everything.
pub async fn nse_corporate_action_proposals(
    client: &NseClient,
    accounts: &[Account],
    assets: &[Asset],
    activities: &[Activity],
    today: NaiveDate,
) -> CorporateActionProposals {
    let symbols = symbols_to_check(accounts, assets, activities, today);
    let mut actions: HashMap<String, Vec<ExchangeAction>> = HashMap::new();
    let mut failed = Vec::new();
    let mut fetched = 0usize;
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        }
        match client.actions_for(symbol).await {
            Ok(list) => {
                fetched += 1;
                actions.insert(symbol.clone(), list);
            }
            Err(error) => {
                failed.push((symbol.clone(), error.to_string()));
                if fetched == 0 && failed.len() >= 3 {
                    for rest in &symbols[index + 1..] {
                        failed.push((
                            rest.clone(),
                            "Skipped: NSE refused the earlier requests.".into(),
                        ));
                    }
                    break;
                }
            }
        }
    }
    let (proposals, already_recorded) =
        propose_corporate_actions(accounts, assets, activities, &actions, today);
    CorporateActionProposals {
        proposals,
        already_recorded,
        symbols_checked: symbols,
        failed_symbols: failed,
    }
}

/// [`nse_corporate_action_proposals`] over every active account.
pub async fn nse_corporate_action_proposals_for_active_accounts(
    account_service: &dyn crate::accounts::AccountServiceTrait,
    asset_service: &dyn crate::assets::AssetServiceTrait,
    activity_service: &dyn crate::activities::ActivityServiceTrait,
) -> crate::Result<CorporateActionProposals> {
    let accounts = account_service.get_active_accounts()?;
    let ids: Vec<String> = accounts.iter().map(|a| a.id.clone()).collect();
    if ids.is_empty() {
        return Ok(CorporateActionProposals::default());
    }
    let activities = activity_service.get_activities_by_account_ids(&ids)?;
    let mut asset_ids: Vec<String> = activities
        .iter()
        .filter_map(|a| a.asset_id.clone())
        .collect();
    asset_ids.sort();
    asset_ids.dedup();
    let assets = asset_service.get_assets_by_asset_ids(&asset_ids).await?;
    let client = NseClient::new()?;
    let today = chrono::Local::now().date_naive();
    Ok(nse_corporate_action_proposals(&client, &accounts, &assets, &activities, today).await)
}

#[cfg(test)]
mod proposals_tests;
