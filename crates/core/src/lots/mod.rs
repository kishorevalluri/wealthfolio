//! Persisted tax lots.
//!
//! A [`LotRecord`] is the durable, relational form of a tax lot: one row per
//! acquisition (or transferred sub-lot), updated in-place as shares are disposed.
//! This is distinct from the in-memory [`crate::portfolio::snapshot::Lot`], which
//! is a computation intermediate produced by the holdings calculator.
//!
//! Lot rows are initially written alongside the existing JSON snapshot path as a
//! parallel record. Quantity mismatches between the two representations are logged
//! at CRITICAL severity so they can be caught before the lots table becomes
//! the authoritative source.
//!
//! Lots that map directly to an activity keep that activity's id in
//! `open_activity_id`. Synthetic sub-lots that do not correspond to an activity
//! row leave it NULL to preserve the foreign-key constraint.

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use crate::errors::Result;
use crate::portfolio::snapshot::AccountStateSnapshot;

pub mod tax_service;
pub use tax_service::{classify_asset_for_indian_tax, compute_indian_tax_summary, extract_fmv_2018};

// ── Repository trait ──────────────────────────────────────────────────────────

/// Records a lot that was fully disposed (remaining_quantity → 0).
///
/// Carries the full lot data so that `sync_lots_for_account` can INSERT the
/// closed lot even if it was never previously written to the database.  This
/// happens during a full recalc/replay: the lot is created and consumed
/// entirely within a single pass, so `extract_lot_records` (which only sees
/// lots still in the in-memory VecDeque) never produces a row for it.
#[derive(Debug, Clone)]
pub struct LotClosure {
    pub lot_id: String,
    /// ISO 8601 date the lot was fully consumed ("YYYY-MM-DD").
    pub close_date: String,
    /// The activity that fully disposed the lot, if known.
    pub close_activity_id: Option<String>,
    /// The activity that originally opened this lot. Carried through so the
    /// closure-insert can preserve the FK link instead of writing NULL.
    /// `None` only for lots that don't correspond to an activity row.
    pub open_activity_id: Option<String>,

    // ── Fields needed to INSERT the lot if it doesn't exist yet ──
    pub account_id: String,
    pub asset_id: String,
    /// ISO 8601 date the lot was opened ("YYYY-MM-DD").
    pub open_date: String,
    /// Quantity when the lot was first created.
    pub original_quantity: String,
    /// Cost per unit in the asset's quote currency.
    pub cost_per_unit: String,
    /// Cost basis at lot creation (cost_per_unit × original_quantity + fee).
    /// Immutable.
    pub original_cost_basis: String,
    /// Original cost basis converted to base currency at acquisition date.
    pub original_cost_basis_base: String,
    /// Remaining cost basis converted with the acquisition-date FX rate.
    pub remaining_cost_basis_base: String,
    /// Transaction fees allocated to this lot.
    pub fee_allocated: String,
    /// Transaction fees converted to base currency at acquisition date.
    pub fee_allocated_base: String,
    /// Trade-level taxes allocated to this lot.
    pub tax_allocated: String,
    /// Trade-level taxes converted to base currency at acquisition date.
    pub tax_allocated_base: String,
    /// Lot currency, normally the asset quote currency.
    pub currency: String,
    /// User base currency used by the base fields.
    pub base_currency: String,
    /// FX rate from lot currency to base currency at acquisition.
    pub fx_rate_to_base: String,
    /// Cost-basis method used when this generated lot row was rebuilt.
    pub cost_basis_method: String,
    /// Cumulative product of post-acquisition SPLIT ratios at the time of
    /// closure. A lot opened before a 2:1 split and fully consumed after the
    /// split should persist with split_ratio = "2", not "1" — otherwise
    /// downstream tax-lot consumers see a misleading split history.
    pub split_ratio: String,
}

/// A deterministic disposal slice produced when a sell consumes a FIFO lot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LotDisposal {
    pub id: String,
    pub lot_id: String,
    pub account_id: String,
    pub asset_id: String,
    pub disposal_activity_id: String,
    pub disposal_date: String,
    pub quantity: String,
    pub proceeds: String,
    pub cost_basis: String,
    pub realized_pnl: String,
    pub proceeds_base: String,
    pub cost_basis_base: String,
    pub realized_pnl_base: String,
    pub currency: String,
    pub base_currency: String,
    pub fx_rate_to_base: String,
    pub cost_basis_method: String,
    pub created_at: String,
}

/// Persistence interface for lot rows.
#[async_trait]
pub trait LotRepositoryTrait: Send + Sync {
    /// Replaces every lot row for the given account with the provided records.
    /// Existing open and closed rows for the account are deleted before insert.
    async fn replace_lots_for_account(&self, account_id: &str, lots: &[LotRecord]) -> Result<()>;

    /// Returns all open (is_closed = 0) lot rows for the given account.
    async fn get_open_lots_for_account(&self, account_id: &str) -> Result<Vec<LotRecord>>;

    /// Returns all open (is_closed = 0) lot rows for the given account and asset.
    async fn get_open_lots_for_account_asset(
        &self,
        account_id: &str,
        asset_id: &str,
    ) -> Result<Vec<LotRecord>> {
        Ok(self
            .get_open_lots_for_account(account_id)
            .await?
            .into_iter()
            .filter(|lot| lot.asset_id == asset_id)
            .collect())
    }

    /// Returns all open (is_closed = 0) lot rows across all accounts.
    async fn get_all_open_lots(&self) -> Result<Vec<LotRecord>>;

    /// Returns all lots that were active on `date` for the specified accounts.
    /// A lot is active if: `open_date <= date AND (is_closed=0 OR close_date > date)`.
    async fn get_lots_as_of_date(
        &self,
        account_ids: &[String],
        date: NaiveDate,
    ) -> Result<Vec<LotRecord>>;

    /// Returns every lot row (open and closed) for the given account.
    /// Callers that need positions at multiple historical dates can fetch once
    /// and filter in memory using the `open_date` / `close_date` fields.
    async fn get_all_lots_for_account(&self, account_id: &str) -> Result<Vec<LotRecord>>;

    /// Returns every lot row (open and closed) for the given asset across all accounts.
    async fn get_lots_for_asset(&self, asset_id: &str) -> Result<Vec<LotRecord>>;

    /// Returns the read-model rows used by the asset Lots view.
    ///
    /// Transaction-derived lots come from the `lots` table. When requested,
    /// latest HOLDINGS-mode snapshot positions are appended as aggregate
    /// rows; they are not persisted as tax lots.
    async fn get_asset_lot_view(
        &self,
        asset_id: &str,
        include_snapshot_positions: bool,
    ) -> Result<Vec<AssetLotView>>;

    /// Returns every lot row (open and closed) across all accounts.
    async fn get_all_lots(&self) -> Result<Vec<LotRecord>>;

    /// Syncs the lots table for the given account while preserving closed history:
    /// - Open lots in `open_lots` are upserted (inserted if new, remaining_quantity updated if changed).
    /// - Existing open lots missing from `open_lots` are removed.
    /// - Lots listed in `closures` are marked is_closed=1 with their close_date/activity.
    ///
    /// Replaces `replace_lots_for_account` once the transition to incremental lot maintenance
    /// is complete.
    async fn sync_lots_for_account(
        &self,
        account_id: &str,
        open_lots: &[LotRecord],
        closures: &[LotClosure],
    ) -> Result<()>;

    /// Synchronizes deterministic disposal slices for an account.
    ///
    /// Full replays pass `replace_all=true` to rebuild the account read model
    /// from scratch. Incremental replays pass the activity ids in the replay
    /// window so stale rows for edited sells are removed without dropping older
    /// realized P&L history.
    async fn sync_lot_disposals_for_account(
        &self,
        _account_id: &str,
        _affected_activity_ids: &[String],
        _disposals: &[LotDisposal],
        _replace_all: bool,
    ) -> Result<()> {
        Ok(())
    }

    /// Returns deterministic disposal slices for an account.
    async fn get_lot_disposals_for_account(&self, _account_id: &str) -> Result<Vec<LotDisposal>> {
        Ok(Vec::new())
    }

    /// Returns deterministic disposal slices for accounts inside the performance period.
    ///
    /// Performance periods use the same convention as valuation flows:
    /// `(start_date, end_date]`.
    async fn get_lot_disposals_for_accounts_in_date_range(
        &self,
        account_ids: &[String],
        start_date_exclusive: NaiveDate,
        end_date_inclusive: NaiveDate,
    ) -> Result<Vec<LotDisposal>> {
        let mut disposals = Vec::new();
        for account_id in account_ids {
            for disposal in self.get_lot_disposals_for_account(account_id).await? {
                let Ok(disposal_date) =
                    NaiveDate::parse_from_str(&disposal.disposal_date, "%Y-%m-%d")
                else {
                    continue;
                };
                if disposal_date > start_date_exclusive && disposal_date <= end_date_inclusive {
                    disposals.push(disposal);
                }
            }
        }
        Ok(disposals)
    }

    /// Synchronous variant for read paths that cannot become async without
    /// changing the public valuation API.
    fn get_lot_disposals_for_accounts_in_date_range_sync(
        &self,
        account_ids: &[String],
        start_date_exclusive: NaiveDate,
        end_date_inclusive: NaiveDate,
    ) -> Result<Vec<LotDisposal>>;

    /// Returns total quantity per asset across all open lots (all accounts).
    /// Used for quote sync planning — determines which assets need price data.
    async fn get_open_position_quantities(&self) -> Result<HashMap<String, Decimal>>;

    /// Returns the total number of lot rows (open and closed) in the lots table.
    fn count_lots(&self) -> Result<i64>;
}

// ── Domain types ──────────────────────────────────────────────────────────────

/// A row in the `lots` table — a persisted tax lot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LotRecord {
    pub id: String,
    pub account_id: String,
    pub asset_id: String,

    /// Date the lot was opened (ISO 8601, e.g. "2024-03-15").
    pub open_date: String,
    /// The activity that created this lot. NULL when the lot originates from a
    /// transferred sub-lot whose ID does not directly correspond to an activity row.
    pub open_activity_id: Option<String>,

    /// Total quantity acquired, in **as-acquired (pre-split)** units. Immutable after insert.
    pub original_quantity: String,
    /// Quantity still held, in **as-acquired (pre-split)** units. Reduced on each disposal.
    /// Effective shares held now = `remaining_quantity * split_ratio`.
    pub remaining_quantity: String,

    /// Cost per unit in the asset's quote currency, in **as-acquired** terms.
    /// Immutable after insert. Adjusted cost per current share = `cost_per_unit / split_ratio`.
    pub cost_per_unit: String,
    /// Cost basis at lot creation (cost_per_unit × original_quantity + fee_allocated).
    /// Immutable. Split-invariant — splits don't change the dollars paid.
    pub original_cost_basis: String,
    /// Open cost basis remaining for the lot. Reduced proportionally on
    /// partial sells: `remaining_cost_basis -= (consumed_qty / original_quantity) × original_cost_basis`.
    /// Reaches zero on full close.
    pub remaining_cost_basis: String,
    /// Original cost basis converted to base currency at acquisition date.
    pub original_cost_basis_base: String,
    /// Open cost basis remaining converted with the acquisition-date FX rate.
    pub remaining_cost_basis_base: String,
    /// Transaction fees allocated to this lot. Immutable.
    pub fee_allocated: String,
    /// Transaction fees converted to base currency at acquisition date.
    pub fee_allocated_base: String,
    /// Trade-level taxes allocated to this lot. Immutable.
    pub tax_allocated: String,
    /// Trade-level taxes converted to base currency at acquisition date.
    pub tax_allocated_base: String,
    /// Lot currency, normally the asset quote currency.
    pub currency: String,
    /// User base currency used by the base fields.
    pub base_currency: String,
    /// FX rate from lot currency to base currency at acquisition.
    pub fx_rate_to_base: String,
    /// FX rate from the lot currency to the account currency at acquisition,
    /// when known. Persisted so the open-lot book reproduces account-currency
    /// cost basis exactly on an append-only rebuild (no market-FX fallback).
    /// `None` for same-currency lots or lots written before this column existed.
    pub fx_rate_to_account: Option<String>,
    /// Account currency that `fx_rate_to_account` converts into. `None`
    /// whenever `fx_rate_to_account` is `None`.
    pub account_currency: Option<String>,
    /// Cost-basis method used when this generated lot row was rebuilt.
    pub cost_basis_method: String,

    /// Cumulative product of post-acquisition SPLIT activity ratios for this lot's asset.
    /// Defaults to "1" (no splits since open_date). Multiplied by `remaining_quantity` to
    /// derive effective current shares; divided into `cost_per_unit` to derive adjusted
    /// per-share basis. See docs/architecture/data_model.md §3.5.
    pub split_ratio: String,

    /// True once remaining_quantity reaches zero.
    pub is_closed: bool,

    /// Date the lot was fully disposed (ISO 8601). None if still open.
    pub close_date: Option<String>,
    /// The activity that fully closed this lot. None if still open.
    pub close_activity_id: Option<String>,

    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetLotSource {
    TransactionLot,
    SnapshotPosition,
}

/// UI-facing lot read model for an asset.
///
/// Snapshot rows are aggregate positions from the latest HOLDINGS-mode account
/// snapshots. They intentionally share the view with real transaction lots
/// without becoming rows in the `lots` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetLotView {
    pub id: String,
    pub account_id: String,
    pub account_name: String,
    pub asset_id: String,
    pub source: AssetLotSource,
    /// Currency for native lot money fields such as cost_basis and unit_cost.
    pub currency: String,
    /// User base currency for *_base fields when available.
    pub base_currency: Option<String>,
    /// Currency used by valuation_* fields after minor-unit normalization.
    pub valuation_currency: String,
    /// Effective current quantity. For transaction lots this is
    /// `remaining_quantity * split_ratio`; snapshot positions are already
    /// aggregate current quantities.
    pub quantity: Decimal,
    /// As-acquired quantity for transaction lots. Snapshot positions report the
    /// aggregate snapshot quantity here for shape consistency.
    pub original_quantity: Decimal,
    /// Remaining quantity in as-acquired units for transaction lots. Snapshot
    /// positions report the aggregate snapshot quantity here.
    pub remaining_quantity: Decimal,
    pub cost_basis: Decimal,
    pub cost_basis_base: Option<Decimal>,
    pub unit_cost: Decimal,
    pub fees: Decimal,
    pub taxes: Decimal,
    pub taxes_base: Option<Decimal>,
    pub valuation_unit_cost: Decimal,
    pub valuation_cost_basis: Decimal,
    pub fx_rate_to_base: Option<Decimal>,
    pub split_ratio: Decimal,
    pub contract_multiplier: Decimal,
    pub acquisition_date: Option<String>,
    pub snapshot_date: Option<String>,
    pub is_closed: bool,
    pub close_date: Option<String>,
    pub disposal_proceeds: Option<Decimal>,
    pub disposal_cost_basis: Option<Decimal>,
    pub disposal_cost_basis_base: Option<Decimal>,
    pub realized_pnl: Option<Decimal>,
    pub realized_pnl_base: Option<Decimal>,
    pub valuation_disposal_cost_basis: Option<Decimal>,
    pub valuation_realized_pnl: Option<Decimal>,
}

// The cost_basis_method field is generation provenance for inventory rows.
// Tax-conclusion concepts (wash sale, holding period, tax character) belong in
// future tax-overlay tables.

// ── Extraction helpers ────────────────────────────────────────────────────────

/// Converts the in-memory lots from a holdings snapshot into [`LotRecord`]s
/// suitable for persisting to the `lots` table.
///
/// Each open lot in every position of the snapshot becomes one row.
/// `open_activity_id` is set from the in-memory `Lot.source_activity_id` so
/// the FK CASCADE removes the row when its activity is deleted. For
/// compiler-generated synthetic activity legs that don't correspond to an
/// activity row, `source_activity_id` is `None` and the column stays NULL.
/// `original_quantity` comes from `lot.original_quantity` when available (new
/// snapshots). For old snapshots that predate the field (where it deserializes
/// as zero), falls back to `lot.quantity` (the remaining amount).
pub fn extract_lot_records(snapshot: &AccountStateSnapshot) -> Vec<LotRecord> {
    extract_lot_records_with_cost_basis_method(snapshot, "FIFO")
}

/// Converts in-memory lots to [`LotRecord`]s and records the cost-basis method
/// used by the generation pass.
pub fn extract_lot_records_with_cost_basis_method(
    snapshot: &AccountStateSnapshot,
    cost_basis_method: &str,
) -> Vec<LotRecord> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let mut records = Vec::new();
    let cost_basis_method = cost_basis_method.trim().to_ascii_uppercase();

    for position in snapshot.positions.values() {
        for lot in &position.lots {
            let orig_qty = if lot.original_quantity.is_zero() {
                lot.quantity
            } else {
                lot.original_quantity
            };
            // Original cost basis (immutable) is reconstructed from at-acquisition
            // values: `acquisition_price` is immutable; `original_fees()` returns
            // the immutable `original_acquisition_fees` (falling back to
            // `acquisition_fees` for pre-this-field snapshots that haven't been
            // partially consumed yet). `lot.cost_basis` is mutated on partial
            // sells and represents the remaining open cost basis.
            let orig_fees = lot.original_fees();
            let orig_taxes = lot.original_taxes();
            let original_cost_basis = lot.acquisition_price * orig_qty + orig_fees + orig_taxes;
            records.push(LotRecord {
                id: lot.id.clone(),
                account_id: snapshot.account_id.clone(),
                asset_id: position.asset_id.clone(),
                open_date: lot.acquisition_date_key().to_string(),
                open_activity_id: lot.source_activity_id.clone(),
                original_quantity: orig_qty.to_string(),
                remaining_quantity: lot.quantity.to_string(),
                cost_per_unit: lot.acquisition_price.to_string(),
                original_cost_basis: original_cost_basis.to_string(),
                remaining_cost_basis: lot.cost_basis.to_string(),
                original_cost_basis_base: original_cost_basis.to_string(),
                remaining_cost_basis_base: lot.cost_basis.to_string(),
                fee_allocated: orig_fees.to_string(),
                fee_allocated_base: orig_fees.to_string(),
                tax_allocated: orig_taxes.to_string(),
                tax_allocated_base: orig_taxes.to_string(),
                currency: position.currency.clone(),
                base_currency: snapshot.currency.clone(),
                fx_rate_to_base: Decimal::ONE.to_string(),
                fx_rate_to_account: lot.fx_rate_to_account.map(|rate| rate.to_string()),
                account_currency: lot.account_currency.clone(),
                cost_basis_method: cost_basis_method.clone(),
                split_ratio: lot.effective_split_ratio().to_string(),
                is_closed: false,
                close_date: None,
                close_activity_id: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
    }

    records
}

/// Reconstructs an in-memory [`crate::portfolio::snapshot::Lot`] from a
/// persisted [`LotRecord`].
///
/// The `lots` table is the source of truth for a lot's *open* state, so the
/// in-memory lot's remaining fields (`quantity`, `cost_basis`) come from the
/// record's `remaining_*` columns, while immutable acquisition-side values
/// (`original_quantity`, `acquisition_price`, allocated fee/tax) come from
/// their immutable columns. Base FX is preserved from `fx_rate_to_base`.
///
/// The account-FX book (`fx_rate_to_account`, `account_currency`) is restored
/// from the persisted `lots.fx_rate_to_account` / `lots.account_currency`
/// columns, so an append-only rebuild reproduces the same account-currency cost
/// basis as a full rebuild for multi-currency accounts (no market-FX fallback).
///
/// `fx_rate_to_position` is not retained per-lot and stays `None`; it only
/// records the activity→position conversion for the audit trail and affects no
/// monetary total (quantity, cost basis, valuation, and realized P&L all flow
/// through `remaining_quantity` / `remaining_cost_basis` / `fx_rate_to_base`).
///
/// `acquisition_fees` / `acquisition_taxes` are set to the immutable allocated
/// amounts (equal to `original_acquisition_fees` / `original_acquisition_taxes`).
/// The table stores only the immutable allocation, not the running
/// proportionally-reduced remainder. This is exact for any lot not partially
/// consumed before it was persisted, and only affects the fee/tax *memo*
/// breakdown (never a monetary total: quantity, cost basis, valuation and
/// realized P&L all flow through `remaining_quantity` / `remaining_cost_basis`).
pub fn lot_record_to_snapshot_lot(
    position_id: &str,
    record: LotRecord,
) -> crate::portfolio::snapshot::Lot {
    // Diagnostics below name the lot and the offending column but never the
    // stored value: these columns carry quantities, cost basis, unit price,
    // fees and taxes, and financial data must not reach the logs. The lot id
    // is enough to locate the row.
    let acquisition_local_date = NaiveDate::parse_from_str(&record.open_date, "%Y-%m-%d").ok();
    if acquisition_local_date.is_none() {
        log::warn!(
            "Lot {} has an unparseable open_date; falling back to the current time. \
             The hydrated lot's acquisition date will be wrong.",
            record.id
        );
    }
    let acquisition_date = acquisition_local_date
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|naive| naive.and_utc())
        .unwrap_or_else(Utc::now);

    let split_ratio = match Decimal::from_str(&record.split_ratio) {
        Ok(ratio) if !ratio.is_zero() => ratio,
        _ => {
            log::warn!(
                "Lot {} has an invalid split_ratio; defaulting to 1.",
                record.id
            );
            Decimal::ONE
        }
    };

    let lot_id = record.id.clone();
    let parse = |field: &str, value: &str| {
        Decimal::from_str(value).unwrap_or_else(|_| {
            log::warn!(
                "Lot {} has an unparseable {}; defaulting to 0.",
                lot_id,
                field
            );
            Decimal::ZERO
        })
    };
    let fees = parse("fee_allocated", &record.fee_allocated);
    let taxes = parse("tax_allocated", &record.tax_allocated);

    crate::portfolio::snapshot::Lot {
        id: record.id,
        position_id: position_id.to_string(),
        acquisition_date,
        acquisition_local_date,
        quantity: parse("remaining_quantity", &record.remaining_quantity),
        original_quantity: parse("original_quantity", &record.original_quantity),
        cost_basis: parse("remaining_cost_basis", &record.remaining_cost_basis),
        acquisition_price: parse("cost_per_unit", &record.cost_per_unit),
        acquisition_fees: fees,
        original_acquisition_fees: fees,
        acquisition_taxes: taxes,
        original_acquisition_taxes: taxes,
        fx_rate_to_position: None,
        fx_rate_to_account: record
            .fx_rate_to_account
            .as_deref()
            .and_then(|value| Decimal::from_str(value).ok()),
        account_currency: record.account_currency,
        fx_rate_to_base: Decimal::from_str(&record.fx_rate_to_base).ok(),
        base_currency: Some(record.base_currency),
        source_activity_id: record.open_activity_id,
        split_ratio,
    }
}

/// Checks that the lot quantities extracted from a snapshot are consistent with
/// the position quantities stored in that same snapshot.
///
/// Any discrepancy is logged at ERROR severity so it can be investigated before
/// the lots table is relied upon for live calculations.
///
/// Returns the number of mismatches found (0 = all consistent).
pub fn check_lot_quantity_consistency(
    snapshot: &AccountStateSnapshot,
    lot_records: &[LotRecord],
) -> usize {
    // Lot quantities are stored in as-acquired (pre-split) units; effective
    // current shares are remaining_quantity * split_ratio. Compare against the
    // position quantity in the same effective-share space.
    let mut lot_qty_by_asset: HashMap<&str, Decimal> = HashMap::new();
    for record in lot_records {
        let qty = Decimal::from_str(&record.remaining_quantity).unwrap_or(Decimal::ZERO);
        let ratio = Decimal::from_str(&record.split_ratio).unwrap_or(Decimal::ONE);
        *lot_qty_by_asset
            .entry(record.asset_id.as_str())
            .or_insert(Decimal::ZERO) += qty * ratio;
    }

    let mut mismatches = 0;
    for (asset_id, position) in &snapshot.positions {
        let lot_qty = lot_qty_by_asset
            .get(asset_id.as_str())
            .copied()
            .unwrap_or(Decimal::ZERO);
        if lot_qty != position.quantity {
            // Name the account, the asset and the failure class, but never the
            // quantities themselves: holdings sizes are financial data and must
            // not reach the logs. The two classes are worth distinguishing
            // because they have different causes — an empty lot book points at
            // extraction or seeding, while a non-empty disagreement points at
            // drift between the two representations.
            let detail = if lot_qty.is_zero() {
                "no lots extracted while the position is non-empty"
            } else {
                "extracted lot quantities disagree with the position"
            };
            log::error!(
                "CRITICAL: lot quantity mismatch for account {} asset {}: {}.",
                snapshot.account_id,
                asset_id,
                detail
            );
            mismatches += 1;
        }
    }
    mismatches
}

// ── Indian Tax Overlay & Capital Gains Calculations ──────────────────────────

/// Classification of Indian capital assets for holding period and tax rate determination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndianAssetTaxClassification {
    /// Listed Indian equity shares and equity-oriented mutual funds (>65% domestic equity).
    /// Holding period: 12 months for LTCG.
    EquityListed,
    /// Equity-oriented mutual funds.
    EquityMf,
    /// Units of listed business trusts (REITs & InvITs). Holding period: 12 months post Budget 2024.
    ReitInvIt,
    /// Specified mutual funds under Section 50AA (acquired on or after April 1, 2023 with <=35% equity).
    /// Deemed short-term capital assets regardless of holding duration.
    DebtMfPost2023,
    /// Unlisted bonds and debentures transferred on or after July 23, 2024 (Section 50AA Finance Act 2024).
    /// Deemed short-term capital assets regardless of holding duration.
    UnlistedDebenturePost2024,
    /// Sovereign Gold Bonds, unlisted equity, debt MFs pre-April 2023, real estate, etc.
    OtherCapitalAsset,
    /// Gold or Sovereign Gold Bonds.
    GoldSgb,
}

/// Character of capital gain based on asset classification and holding period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldingPeriodCategory {
    ShortTerm,
    LongTerm,
}

/// Calculates deemed cost of acquisition under Section 55(2)(ac) (Grandfathering for listed equity acquired on or before 31-Jan-2018).
///
/// Formula:
/// Deemed Cost = max(Actual Cost, min(FMV on 31-Jan-2018, Sale Price))
///
/// If acquisition date is after 2018-01-31 or FMV is None, returns `actual_cost`.
pub fn calculate_grandfathered_cost_basis(
    acquisition_date: NaiveDate,
    actual_cost: Decimal,
    fmv_2018_01_31: Option<Decimal>,
    sale_price: Decimal,
) -> Decimal {
    let cutoff = NaiveDate::from_ymd_opt(2018, 1, 31).unwrap();
    if acquisition_date > cutoff {
        return actual_cost;
    }

    match fmv_2018_01_31 {
        Some(fmv) => {
            let lower_of_fmv_and_sale = fmv.min(sale_price);
            actual_cost.max(lower_of_fmv_and_sale)
        }
        None => actual_cost,
    }
}

/// Returns statutory Indian equity tax rates and Section 112A threshold for a given disposal date.
///
/// Returns: (stcg_rate, ltcg_rate, sec_112a_exemption_limit)
///
/// Boundary:
/// - Disposals before 23-Jul-2024 (Budget 2024): STCG 15% (0.15), LTCG 10% (0.10), Sec 112A ₹1,00,000 (100000).
/// - Disposals on or after 23-Jul-2024: STCG 20% (0.20), LTCG 12.5% (0.125), Sec 112A ₹1,25,000 (125000).
pub fn indian_equity_tax_rates_for_date(disposal_date: NaiveDate) -> (Decimal, Decimal, Decimal) {
    let budget_2024_date = NaiveDate::from_ymd_opt(2024, 7, 23).unwrap();
    if disposal_date < budget_2024_date {
        (
            Decimal::new(15, 2),
            Decimal::new(10, 2),
            Decimal::new(100000, 0),
        )
    } else {
        (
            Decimal::new(20, 2),
            Decimal::new(125, 3), // 0.125
            Decimal::new(125000, 0),
        )
    }
}

/// Determines whether a capital gain is Short-Term or Long-Term under Indian Income Tax provisions.
pub fn determine_holding_period_category(
    acquisition_date: NaiveDate,
    disposal_date: NaiveDate,
    classification: IndianAssetTaxClassification,
) -> HoldingPeriodCategory {
    // Section 50AA: specified debt mutual funds and unlisted debentures are deemed short-term capital assets
    if classification == IndianAssetTaxClassification::DebtMfPost2023
        || classification == IndianAssetTaxClassification::UnlistedDebenturePost2024
    {
        return HoldingPeriodCategory::ShortTerm;
    }

    // Days difference calculation
    let days = (disposal_date - acquisition_date).num_days();

    match classification {
        IndianAssetTaxClassification::EquityListed
        | IndianAssetTaxClassification::EquityMf
        | IndianAssetTaxClassification::ReitInvIt => {
            // > 12 months (365 days) is Long Term
            if days > 365 {
                HoldingPeriodCategory::LongTerm
            } else {
                HoldingPeriodCategory::ShortTerm
            }
        }
        IndianAssetTaxClassification::OtherCapitalAsset | IndianAssetTaxClassification::GoldSgb => {
            // Post Budget 2024, holding period for most listed/unlisted and gold standardized to 24 months (730 days)
            if days > 730 {
                HoldingPeriodCategory::LongTerm
            } else {
                HoldingPeriodCategory::ShortTerm
            }
        }
        IndianAssetTaxClassification::DebtMfPost2023
        | IndianAssetTaxClassification::UnlistedDebenturePost2024 => {
            HoldingPeriodCategory::ShortTerm
        }
    }
}

/// Individual lot disposal tax calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianCapitalGainCalculation {
    pub disposal_id: String,
    pub lot_id: String,
    pub asset_id: String,
    pub classification: IndianAssetTaxClassification,
    pub acquisition_date: NaiveDate,
    pub disposal_date: NaiveDate,
    pub holding_period: HoldingPeriodCategory,
    pub actual_cost: Decimal,
    pub deemed_cost: Decimal,
    pub proceeds: Decimal,
    pub gross_gain: Decimal,
    pub taxable_gain: Decimal,
    pub applicable_tax_rate: Decimal,
    pub estimated_tax: Decimal,
}

/// Annual Indian Tax Summary with Section 112A exemption pooling, loss set-offs, and pre/post Budget 2024 pools
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianTaxYearSummary {
    /// Pre-July 23, 2024 Section 111A STCG (@ 15%)
    pub stcg_111a_pre_budget: Decimal,
    /// Post-July 23, 2024 Section 111A STCG (@ 20%)
    pub stcg_111a_post_budget: Decimal,
    /// STCG taxable at individual slab rates (Section 50AA, unlisted securities, etc.)
    pub stcg_slab_total: Decimal,
    /// Total STCG before loss set-off
    pub stcg_total: Decimal,
    /// Pre-July 23, 2024 Section 112A LTCG (@ 10%)
    pub ltcg_112a_pre_budget: Decimal,
    /// Post-July 23, 2024 Section 112A LTCG (@ 12.5%)
    pub ltcg_112a_post_budget: Decimal,
    /// Total gross Section 112A LTCG
    pub sec_112a_ltcg_gross: Decimal,
    /// Section 112A exemption applied
    pub sec_112a_exemption_applied: Decimal,
    /// Net taxable Section 112A LTCG after exemption
    pub sec_112a_ltcg_taxable: Decimal,
    /// Other LTCG taxable under Section 112 (12.5% / 20%)
    pub ltcg_other_total: Decimal,
    /// Total LTCG before loss set-off
    pub ltcg_total: Decimal,
    /// Unabsorbed short-term capital loss to carry forward
    pub stcl_carried_forward: Decimal,
    /// Unabsorbed long-term capital loss to carry forward
    pub ltcl_carried_forward: Decimal,
    /// Estimated tax on short-term gains (special rate buckets)
    pub estimated_stcg_tax: Decimal,
    /// Estimated tax on long-term gains
    pub estimated_ltcg_tax: Decimal,
    pub calculations: Vec<IndianCapitalGainCalculation>,
}

/// Calculates Indian capital gains for a series of lot disposals, handling grandfathering under
/// Section 55(2)(ac), Budget 2024 rate cut-offs, Section 112A exemption pooling, and statutory loss set-offs (Sec 70/71).
pub fn calculate_indian_capital_gains(
    disposals: &[LotDisposal],
    lot_open_dates: &std::collections::HashMap<String, NaiveDate>,
    asset_classifications: &std::collections::HashMap<String, IndianAssetTaxClassification>,
    asset_fmv_2018: &std::collections::HashMap<String, Decimal>,
) -> IndianTaxYearSummary {
    let mut summary = IndianTaxYearSummary::default();
    let budget_2024_date = NaiveDate::from_ymd_opt(2024, 7, 23).unwrap();

    // Determine the relevant Section 112A exemption limit based on the latest disposal date (or default post-budget)
    let max_disposal_date = disposals
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(&d.disposal_date[..10], "%Y-%m-%d").ok())
        .max()
        .unwrap_or(budget_2024_date);

    let (_, _, max_sec_112a_exemption) = indian_equity_tax_rates_for_date(max_disposal_date);

    let mut sec_112a_pre_pool = Decimal::ZERO;
    let mut sec_112a_post_pool = Decimal::ZERO;

    for d in disposals {
        let disp_date = match NaiveDate::parse_from_str(&d.disposal_date[..10], "%Y-%m-%d") {
            Ok(dt) => dt,
            Err(_) => continue,
        };
        let acq_date = lot_open_dates
            .get(&d.lot_id)
            .copied()
            .unwrap_or(disp_date);

        let classification = asset_classifications
            .get(&d.asset_id)
            .copied()
            .unwrap_or(IndianAssetTaxClassification::OtherCapitalAsset);

        let cost_basis: Decimal = d.cost_basis_base.parse().unwrap_or(Decimal::ZERO);
        let proceeds: Decimal = d.proceeds_base.parse().unwrap_or(Decimal::ZERO);

        let fmv_2018 = asset_fmv_2018.get(&d.asset_id).copied();

        // Calculate grandfathered cost basis if applicable under Section 55(2)(ac)
        let deemed_cost = if classification == IndianAssetTaxClassification::EquityListed
            || classification == IndianAssetTaxClassification::EquityMf
            || classification == IndianAssetTaxClassification::ReitInvIt
        {
            calculate_grandfathered_cost_basis(acq_date, cost_basis, fmv_2018, proceeds)
        } else {
            cost_basis
        };

        let gross_gain = proceeds - deemed_cost;
        let holding_period = determine_holding_period_category(acq_date, disp_date, classification);
        let (stcg_rate, ltcg_rate, _) = indian_equity_tax_rates_for_date(disp_date);

        let is_sec_112a_eligible = (classification == IndianAssetTaxClassification::EquityListed
            || classification == IndianAssetTaxClassification::EquityMf
            || classification == IndianAssetTaxClassification::ReitInvIt)
            && holding_period == HoldingPeriodCategory::LongTerm;

        let is_sec_111a_eligible = (classification == IndianAssetTaxClassification::EquityListed
            || classification == IndianAssetTaxClassification::EquityMf
            || classification == IndianAssetTaxClassification::ReitInvIt)
            && holding_period == HoldingPeriodCategory::ShortTerm;

        let applicable_rate = match holding_period {
            HoldingPeriodCategory::ShortTerm => stcg_rate,
            HoldingPeriodCategory::LongTerm => ltcg_rate,
        };

        if is_sec_112a_eligible {
            if disp_date < budget_2024_date {
                summary.ltcg_112a_pre_budget += gross_gain;
                if gross_gain > Decimal::ZERO {
                    sec_112a_pre_pool += gross_gain;
                }
            } else {
                summary.ltcg_112a_post_budget += gross_gain;
                if gross_gain > Decimal::ZERO {
                    sec_112a_post_pool += gross_gain;
                }
            }
            summary.sec_112a_ltcg_gross += gross_gain;
        } else if holding_period == HoldingPeriodCategory::LongTerm {
            summary.ltcg_other_total += gross_gain;
        }

        match holding_period {
            HoldingPeriodCategory::ShortTerm => {
                summary.stcg_total += gross_gain;
                if is_sec_111a_eligible {
                    if disp_date < budget_2024_date {
                        summary.stcg_111a_pre_budget += gross_gain;
                        if gross_gain > Decimal::ZERO {
                            summary.estimated_stcg_tax += gross_gain * Decimal::new(15, 2);
                        }
                    } else {
                        summary.stcg_111a_post_budget += gross_gain;
                        if gross_gain > Decimal::ZERO {
                            summary.estimated_stcg_tax += gross_gain * Decimal::new(20, 2);
                        }
                    }
                } else {
                    summary.stcg_slab_total += gross_gain;
                }
            }
            HoldingPeriodCategory::LongTerm => {
                summary.ltcg_total += gross_gain;
            }
        }

        summary.calculations.push(IndianCapitalGainCalculation {
            disposal_id: d.id.clone(),
            lot_id: d.lot_id.clone(),
            asset_id: d.asset_id.clone(),
            classification,
            acquisition_date: acq_date,
            disposal_date: disp_date,
            holding_period,
            actual_cost: cost_basis,
            deemed_cost,
            proceeds,
            gross_gain,
            taxable_gain: gross_gain,
            applicable_tax_rate: applicable_rate,
            estimated_tax: Decimal::ZERO,
        });
    }

    // Apply Section 112A aggregate exemption proportionally / sequentially (pre-budget pool first)
    let total_sec_112a_gain = sec_112a_pre_pool + sec_112a_post_pool;
    let exemption_applied = total_sec_112a_gain.min(max_sec_112a_exemption);
    summary.sec_112a_exemption_applied = exemption_applied;
    summary.sec_112a_ltcg_taxable = (total_sec_112a_gain - exemption_applied).max(Decimal::ZERO);

    // Exempt pre-budget gains first, then post-budget gains
    let mut remaining_exemption = exemption_applied;
    let taxable_pre = if sec_112a_pre_pool >= remaining_exemption {
        let taxable = sec_112a_pre_pool - remaining_exemption;
        remaining_exemption = Decimal::ZERO;
        taxable
    } else {
        remaining_exemption -= sec_112a_pre_pool;
        Decimal::ZERO
    };

    let taxable_post = (sec_112a_post_pool - remaining_exemption).max(Decimal::ZERO);

    // Calculate exact LTCG tax across pre (10%) and post (12.5%) pools
    summary.estimated_ltcg_tax = (taxable_pre * Decimal::new(10, 2)) + (taxable_post * Decimal::new(125, 3));

    // Handle Capital Loss Carry-Forward tracking under Section 74
    if summary.stcg_total < Decimal::ZERO {
        summary.stcl_carried_forward = summary.stcg_total.abs();
    }
    if summary.ltcg_total < Decimal::ZERO {
        summary.ltcl_carried_forward = summary.ltcg_total.abs();
    }

    summary
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::snapshot::{AccountStateSnapshot, Lot, Position};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;
    use std::collections::{HashMap, VecDeque};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_lot(
        id: &str,
        position_id: &str,
        date_ymd: (i32, u32, u32),
        qty: Decimal,
        price: Decimal,
        fee: Decimal,
    ) -> Lot {
        Lot {
            id: id.to_string(),
            position_id: position_id.to_string(),
            acquisition_date: Utc
                .with_ymd_and_hms(date_ymd.0, date_ymd.1, date_ymd.2, 0, 0, 0)
                .unwrap(),
            acquisition_local_date: None,
            quantity: qty,
            original_quantity: qty,
            cost_basis: qty * price + fee,
            acquisition_price: price,
            acquisition_fees: fee,
            original_acquisition_fees: fee,
            acquisition_taxes: Decimal::ZERO,
            original_acquisition_taxes: Decimal::ZERO,
            fx_rate_to_position: None,
            fx_rate_to_account: None,
            account_currency: None,
            fx_rate_to_base: None,
            base_currency: None,
            source_activity_id: None,
            split_ratio: Decimal::ONE,
        }
    }

    fn make_position(account_id: &str, asset_id: &str, currency: &str, lots: Vec<Lot>) -> Position {
        let mut pos = Position::new(
            account_id.to_string(),
            asset_id.to_string(),
            currency.to_string(),
            Utc::now(),
        );
        pos.lots = VecDeque::from(lots);
        pos.recalculate_aggregates();
        pos
    }

    fn make_snapshot(
        account_id: &str,
        positions: HashMap<String, Position>,
    ) -> AccountStateSnapshot {
        AccountStateSnapshot {
            id: format!("{}_test", account_id),
            account_id: account_id.to_string(),
            snapshot_date: chrono::NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            currency: "USD".to_string(),
            positions,
            calculated_at: Utc::now().naive_utc(),
            ..Default::default()
        }
    }

    // ── extract_lot_records ───────────────────────────────────────────────────

    /// AAPL with 3 lots from different purchase dates — verifies multi-lot
    /// aggregation and field mapping.
    #[test]
    fn extract_lot_records_aapl_three_lots() {
        let lots = vec![
            make_lot(
                "buy-aapl-1",
                "POS-AAPL-acc1",
                (2024, 1, 15),
                dec!(50),
                dec!(185.00),
                dec!(0),
            ),
            make_lot(
                "buy-aapl-2",
                "POS-AAPL-acc1",
                (2024, 6, 1),
                dec!(30),
                dec!(192.50),
                dec!(0),
            ),
            make_lot(
                "buy-aapl-3",
                "POS-AAPL-acc1",
                (2024, 10, 15),
                dec!(20),
                dec!(225.00),
                dec!(0),
            ),
        ];
        let pos = make_position("acc1", "AAPL", "USD", lots);
        assert_eq!(pos.quantity, dec!(100));

        let mut positions = HashMap::new();
        positions.insert("AAPL".to_string(), pos);
        let snap = make_snapshot("acc1", positions);

        let records = extract_lot_records(&snap);

        assert_eq!(records.len(), 3);
        let total_qty: Decimal = records
            .iter()
            .map(|r| r.remaining_quantity.parse::<Decimal>().unwrap())
            .sum();
        assert_eq!(total_qty, dec!(100));

        for r in &records {
            assert_eq!(r.account_id, "acc1");
            assert_eq!(r.asset_id, "AAPL");
            assert_eq!(r.cost_basis_method, "FIFO");
            assert!(r.open_activity_id.is_none());
            assert!(!r.is_closed);
        }

        // Spot-check first lot
        let r1 = records.iter().find(|r| r.id == "buy-aapl-1").unwrap();
        assert_eq!(r1.remaining_quantity.parse::<Decimal>().unwrap(), dec!(50));
        assert_eq!(r1.cost_per_unit.parse::<Decimal>().unwrap(), dec!(185.00));
        assert_eq!(r1.open_date, "2024-01-15");
    }

    /// LQD bond ETF with 2 lots — verifies correct handling of bond-like symbols.
    #[test]
    fn extract_lot_records_lqd_two_lots() {
        let lots = vec![
            make_lot(
                "buy-lqd-1",
                "POS-LQD-acc1",
                (2024, 2, 1),
                dec!(100),
                dec!(107.25),
                dec!(0),
            ),
            make_lot(
                "buy-lqd-2",
                "POS-LQD-acc1",
                (2024, 8, 15),
                dec!(50),
                dec!(112.10),
                dec!(0),
            ),
        ];
        let pos = make_position("acc1", "LQD", "USD", lots);
        assert_eq!(pos.quantity, dec!(150));

        let mut positions = HashMap::new();
        positions.insert("LQD".to_string(), pos);
        let snap = make_snapshot("acc1", positions);

        let records = extract_lot_records(&snap);
        assert_eq!(records.len(), 2);

        let total_qty: Decimal = records
            .iter()
            .map(|r| r.remaining_quantity.parse::<Decimal>().unwrap())
            .sum();
        assert_eq!(total_qty, dec!(150));
    }

    /// Regression: a lot that has been partially consumed must still persist
    /// the immutable `original_cost_basis` and `fee_allocated` it had at
    /// acquisition. Before adding `original_acquisition_fees`,
    /// `extract_lot_records` reconstructed those values from the mutated
    /// `acquisition_fees` field, silently corrupting the new lot table on
    /// every partial sell.
    #[test]
    fn extract_lot_records_preserves_original_fees_after_partial_sell() {
        // Lot bought 100 shares @ $15 with a $10 fee. Partially consumed:
        // remaining = 50, cost_basis halved, acquisition_fees halved on the
        // in-memory side. The persisted record must still report the original
        // fee of $10 and an original_cost_basis of 100*$15 + $10 = $1510.
        let mut lot = make_lot(
            "buy-1",
            "POS-XYZ-acc1",
            (2024, 1, 1),
            dec!(100),
            dec!(15),
            dec!(10),
        );
        lot.quantity = dec!(50);
        lot.cost_basis = dec!(750);
        lot.acquisition_fees = dec!(5); // mutated by reduce_lots_fifo

        let pos = make_position("acc1", "XYZ", "USD", vec![lot]);
        let mut positions = HashMap::new();
        positions.insert("XYZ".to_string(), pos);
        let snap = make_snapshot("acc1", positions);

        let records = extract_lot_records(&snap);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.remaining_quantity.parse::<Decimal>().unwrap(), dec!(50));
        assert_eq!(r.original_quantity.parse::<Decimal>().unwrap(), dec!(100));
        assert_eq!(r.fee_allocated.parse::<Decimal>().unwrap(), dec!(10));
        assert_eq!(
            r.original_cost_basis.parse::<Decimal>().unwrap(),
            dec!(1510)
        );
        assert_eq!(
            r.remaining_cost_basis.parse::<Decimal>().unwrap(),
            dec!(750)
        );
    }

    /// AAPL Jun 2026 $200 call option — verifies options symbols are handled
    /// the same as any other asset_id.
    #[test]
    fn extract_lot_records_aapl_option_single_lot() {
        let symbol = "AAPL260619C00200000";
        let lots = vec![make_lot(
            "buy-opt-1",
            &format!("POS-{}-acc1", symbol),
            (2025, 11, 1),
            dec!(5),
            dec!(8.50),
            dec!(0),
        )];
        let pos = make_position("acc1", symbol, "USD", lots);

        let mut positions = HashMap::new();
        positions.insert(symbol.to_string(), pos);
        let snap = make_snapshot("acc1", positions);

        let records = extract_lot_records(&snap);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].asset_id, symbol);
        assert_eq!(
            records[0].remaining_quantity.parse::<Decimal>().unwrap(),
            dec!(5)
        );
    }

    /// Multi-asset portfolio: AAPL (3 lots) + LQD (2 lots) + option (1 lot).
    #[test]
    fn extract_lot_records_mixed_portfolio() {
        let mut positions = HashMap::new();

        positions.insert(
            "AAPL".to_string(),
            make_position(
                "acc1",
                "AAPL",
                "USD",
                vec![
                    make_lot(
                        "buy-aapl-1",
                        "POS-AAPL-acc1",
                        (2024, 1, 15),
                        dec!(50),
                        dec!(185.00),
                        dec!(0),
                    ),
                    make_lot(
                        "buy-aapl-2",
                        "POS-AAPL-acc1",
                        (2024, 6, 1),
                        dec!(30),
                        dec!(192.50),
                        dec!(0),
                    ),
                    make_lot(
                        "buy-aapl-3",
                        "POS-AAPL-acc1",
                        (2024, 10, 15),
                        dec!(20),
                        dec!(225.00),
                        dec!(0),
                    ),
                ],
            ),
        );
        positions.insert(
            "LQD".to_string(),
            make_position(
                "acc1",
                "LQD",
                "USD",
                vec![
                    make_lot(
                        "buy-lqd-1",
                        "POS-LQD-acc1",
                        (2024, 2, 1),
                        dec!(100),
                        dec!(107.25),
                        dec!(0),
                    ),
                    make_lot(
                        "buy-lqd-2",
                        "POS-LQD-acc1",
                        (2024, 8, 15),
                        dec!(50),
                        dec!(112.10),
                        dec!(0),
                    ),
                ],
            ),
        );
        positions.insert(
            "AAPL260619C00200000".to_string(),
            make_position(
                "acc1",
                "AAPL260619C00200000",
                "USD",
                vec![make_lot(
                    "buy-opt-1",
                    "POS-AAPL260619C00200000-acc1",
                    (2025, 11, 1),
                    dec!(5),
                    dec!(8.50),
                    dec!(0),
                )],
            ),
        );

        let snap = make_snapshot("acc1", positions);
        let records = extract_lot_records(&snap);

        assert_eq!(records.len(), 6);

        let aapl_qty: Decimal = records
            .iter()
            .filter(|r| r.asset_id == "AAPL")
            .map(|r| r.remaining_quantity.parse::<Decimal>().unwrap())
            .sum();
        assert_eq!(aapl_qty, dec!(100));

        let lqd_qty: Decimal = records
            .iter()
            .filter(|r| r.asset_id == "LQD")
            .map(|r| r.remaining_quantity.parse::<Decimal>().unwrap())
            .sum();
        assert_eq!(lqd_qty, dec!(150));

        let opt_qty: Decimal = records
            .iter()
            .filter(|r| r.asset_id == "AAPL260619C00200000")
            .map(|r| r.remaining_quantity.parse::<Decimal>().unwrap())
            .sum();
        assert_eq!(opt_qty, dec!(5));
    }

    // ── check_lot_quantity_consistency ────────────────────────────────────────

    #[test]
    fn consistency_check_passes_when_quantities_match() {
        let mut positions = HashMap::new();
        positions.insert(
            "AAPL".to_string(),
            make_position(
                "acc1",
                "AAPL",
                "USD",
                vec![
                    make_lot(
                        "l1",
                        "POS-AAPL-acc1",
                        (2024, 1, 15),
                        dec!(50),
                        dec!(185),
                        dec!(0),
                    ),
                    make_lot(
                        "l2",
                        "POS-AAPL-acc1",
                        (2024, 6, 1),
                        dec!(50),
                        dec!(192),
                        dec!(0),
                    ),
                ],
            ),
        );
        let snap = make_snapshot("acc1", positions);
        let records = extract_lot_records(&snap);

        let mismatches = check_lot_quantity_consistency(&snap, &records);
        assert_eq!(mismatches, 0);
    }

    #[test]
    fn consistency_check_detects_quantity_mismatch() {
        // Build a snapshot where position.quantity says 100 but the lot records only sum to 50.
        let mut positions = HashMap::new();
        let mut pos = make_position(
            "acc1",
            "AAPL",
            "USD",
            vec![
                make_lot(
                    "l1",
                    "POS-AAPL-acc1",
                    (2024, 1, 15),
                    dec!(50),
                    dec!(185),
                    dec!(0),
                ),
                make_lot(
                    "l2",
                    "POS-AAPL-acc1",
                    (2024, 6, 1),
                    dec!(50),
                    dec!(192),
                    dec!(0),
                ),
            ],
        );
        // Manually inflate the position quantity to create a mismatch.
        pos.quantity = dec!(100);
        positions.insert("AAPL".to_string(), pos);
        let snap = make_snapshot("acc1", positions);

        // Build lot records that only total 50.
        let partial_records = vec![LotRecord {
            id: "l1".to_string(),
            account_id: "acc1".to_string(),
            asset_id: "AAPL".to_string(),
            open_date: "2024-01-15".to_string(),
            open_activity_id: None,
            original_quantity: "50".to_string(),
            remaining_quantity: "50".to_string(),
            cost_per_unit: "185".to_string(),
            original_cost_basis: "9250".to_string(),
            remaining_cost_basis: "9250".to_string(),
            original_cost_basis_base: "9250".to_string(),
            remaining_cost_basis_base: "9250".to_string(),
            fee_allocated: "0".to_string(),
            fee_allocated_base: "0".to_string(),
            tax_allocated: "0".to_string(),
            tax_allocated_base: "0".to_string(),
            currency: "USD".to_string(),
            base_currency: "USD".to_string(),
            fx_rate_to_base: "1".to_string(),
            fx_rate_to_account: None,
            account_currency: None,
            cost_basis_method: "FIFO".to_string(),
            split_ratio: "1".to_string(),
            is_closed: false,
            close_date: None,
            close_activity_id: None,
            created_at: "2024-01-15T00:00:00.000Z".to_string(),
            updated_at: "2024-01-15T00:00:00.000Z".to_string(),
        }];

        let mismatches = check_lot_quantity_consistency(&snap, &partial_records);
        assert_eq!(mismatches, 1);
    }

    // ── Indian Tax Tests ───────────────────────────────────────────────────────

    #[test]
    fn test_budget_2024_tax_rates_and_limits() {
        let pre_budget = NaiveDate::from_ymd_opt(2024, 7, 22).unwrap();
        let post_budget = NaiveDate::from_ymd_opt(2024, 7, 23).unwrap();

        let (stcg_pre, ltcg_pre, limit_pre) = indian_equity_tax_rates_for_date(pre_budget);
        assert_eq!(stcg_pre, dec!(0.15));
        assert_eq!(ltcg_pre, dec!(0.10));
        assert_eq!(limit_pre, dec!(100000));

        let (stcg_post, ltcg_post, limit_post) = indian_equity_tax_rates_for_date(post_budget);
        assert_eq!(stcg_post, dec!(0.20));
        assert_eq!(ltcg_post, dec!(0.125));
        assert_eq!(limit_post, dec!(125000));
    }

    #[test]
    fn test_section_55_2_ac_grandfathering_cases() {
        let pre_2018 = NaiveDate::from_ymd_opt(2017, 5, 10).unwrap();
        let post_2018 = NaiveDate::from_ymd_opt(2018, 2, 1).unwrap();

        // Case 1: Acquired after Jan 31, 2018 -> grandfathering does not apply
        let basis1 = calculate_grandfathered_cost_basis(post_2018, dec!(100), Some(dec!(150)), dec!(200));
        assert_eq!(basis1, dec!(100));

        // Case 2: Actual cost (100) < FMV (150) < Sale price (200)
        // Lower of (FMV, Sale) = 150. Max(100, 150) = 150. Gain taxed = 200 - 150 = 50.
        let basis2 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(150)), dec!(200));
        assert_eq!(basis2, dec!(150));

        // Case 3: Actual cost (100) < Sale price (120) < FMV (150)
        // Lower of (FMV, Sale) = 120. Max(100, 120) = 120. Gain taxed = 120 - 120 = 0 (No artificial loss).
        let basis3 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(150)), dec!(120));
        assert_eq!(basis3, dec!(120));

        // Case 4: Sale price (80) < FMV (150), Actual cost = 100
        // Lower of (FMV, Sale) = 80. Max(100, 80) = 100. Loss = 80 - 100 = -20.
        let basis4 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(150)), dec!(80));
        assert_eq!(basis4, dec!(100));

        // Case 5: FMV (80) < Actual cost (100) < Sale price (200)
        // Lower of (FMV, Sale) = 80. Max(100, 80) = 100. Basis remains 100.
        let basis5 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(80)), dec!(200));
        assert_eq!(basis5, dec!(100));
    }

    #[test]
    fn test_section_50aa_debt_mf_always_short_term() {
        let acq = NaiveDate::from_ymd_opt(2023, 4, 15).unwrap();
        let disp = NaiveDate::from_ymd_opt(2025, 6, 1).unwrap(); // > 2 years holding

        // Standard equity would be LongTerm
        let equity_cat = determine_holding_period_category(acq, disp, IndianAssetTaxClassification::EquityListed);
        assert_eq!(equity_cat, HoldingPeriodCategory::LongTerm);

        // Section 50AA Debt MF is deemed ShortTerm regardless of duration
        let debt_cat = determine_holding_period_category(acq, disp, IndianAssetTaxClassification::DebtMfPost2023);
        assert_eq!(debt_cat, HoldingPeriodCategory::ShortTerm);
    }

    #[test]
    fn test_calculate_indian_capital_gains_and_112a_pooling() {
        // Lot 1: Pre-2018 equity sold post Budget 2024
        // Acq: 2017-01-01, Disp: 2024-08-01 (>12m -> LTCG)
        // Actual cost = 100,000, FMV 2018 = 150,000, Sale = 300,000
        // Grandfathered deemed cost = 150,000 -> LTCG = 150,000
        let disp1 = LotDisposal {
            id: "disp-1".to_string(),
            lot_id: "lot-1".to_string(),
            account_id: "acc-1".to_string(),
            asset_id: "RELIANCE".to_string(),
            disposal_activity_id: "act-1".to_string(),
            disposal_date: "2024-08-01T00:00:00.000Z".to_string(),
            quantity: "100".to_string(),
            proceeds: "300000".to_string(),
            cost_basis: "100000".to_string(),
            realized_pnl: "200000".to_string(),
            proceeds_base: "300000".to_string(),
            cost_basis_base: "100000".to_string(),
            realized_pnl_base: "200000".to_string(),
            currency: "INR".to_string(),
            base_currency: "INR".to_string(),
            fx_rate_to_base: "1".to_string(),
            cost_basis_method: "FIFO".to_string(),
            created_at: "2024-08-01T00:00:00.000Z".to_string(),
        };

        // Lot 2: Equity sold within 6 months post Budget 2024 (STCG)
        // Cost = 50,000, Sale = 70,000 -> Gain = 20,000 @ 20%
        let disp2 = LotDisposal {
            id: "disp-2".to_string(),
            lot_id: "lot-2".to_string(),
            account_id: "acc-1".to_string(),
            asset_id: "TCS".to_string(),
            disposal_activity_id: "act-2".to_string(),
            disposal_date: "2024-08-05T00:00:00.000Z".to_string(),
            quantity: "10".to_string(),
            proceeds: "70000".to_string(),
            cost_basis: "50000".to_string(),
            realized_pnl: "20000".to_string(),
            proceeds_base: "70000".to_string(),
            cost_basis_base: "50000".to_string(),
            realized_pnl_base: "20000".to_string(),
            currency: "INR".to_string(),
            base_currency: "INR".to_string(),
            fx_rate_to_base: "1".to_string(),
            cost_basis_method: "FIFO".to_string(),
            created_at: "2024-08-05T00:00:00.000Z".to_string(),
        };

        let mut lot_open_dates = HashMap::new();
        lot_open_dates.insert("lot-1".to_string(), NaiveDate::from_ymd_opt(2017, 1, 1).unwrap());
        lot_open_dates.insert("lot-2".to_string(), NaiveDate::from_ymd_opt(2024, 3, 1).unwrap());

        let mut asset_classifications = HashMap::new();
        asset_classifications.insert("RELIANCE".to_string(), IndianAssetTaxClassification::EquityListed);
        asset_classifications.insert("TCS".to_string(), IndianAssetTaxClassification::EquityListed);

        let mut asset_fmv_2018 = HashMap::new();
        asset_fmv_2018.insert("RELIANCE".to_string(), dec!(150000));

        let summary = calculate_indian_capital_gains(
            &[disp1, disp2],
            &lot_open_dates,
            &asset_classifications,
            &asset_fmv_2018,
        );

        // STCG should be 20,000, tax at 20% = 4,000
        assert_eq!(summary.stcg_total, dec!(20000));
        assert_eq!(summary.estimated_stcg_tax, dec!(4000));

        // LTCG gross = 150,000 (after 150,000 grandfathering deemed cost)
        assert_eq!(summary.sec_112a_ltcg_gross, dec!(150000));
        // Sec 112A exemption = 125,000
        assert_eq!(summary.sec_112a_exemption_applied, dec!(125000));
        // Taxable LTCG = 150,000 - 125,000 = 25,000
        assert_eq!(summary.sec_112a_ltcg_taxable, dec!(25000));
        // LTCG tax at 12.5% on 25,000 = 3,125
        assert_eq!(summary.estimated_ltcg_tax, dec!(3125));
    }
}
