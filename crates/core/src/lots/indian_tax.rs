//! Indian capital gains overlay on lot disposals.
//!
//! Classifies each FIFO disposal into a tax section (111A, 112A, 112, 50AA or
//! slab-rate STCG), applies Section 55(2)(ac) grandfathering, sets off losses
//! under Section 70 *before* the Section 112A exemption, and estimates the tax
//! at the special rates in force on each sale date.
//!
//! The estimate covers special-rate gains only. Slab-rate STCG is reported as
//! an amount to add to the person's income. Surcharge, rebate and the basic
//! exemption adjustment depend on total income, so they are worked out on the
//! tax page from `special_rate_gains` and the income the person enters.

use chrono::{Months, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::LotDisposal;

/// Classification of Indian capital assets for holding period and tax rate determination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndianAssetTaxClassification {
    /// Listed Indian equity shares (STT paid).
    EquityListed,
    /// Equity-oriented mutual funds and equity ETFs (STT paid).
    EquityMf,
    /// Units of listed business trusts (REITs and InvITs).
    ReitInvIt,
    /// Mutual funds that are not equity-oriented (debt, gold, international,
    /// most FoFs). Units bought on or after 1 Apr 2023 fall under Section 50AA:
    /// any gain is short-term and taxed at slab rate. Older units follow the
    /// general capital-asset rules.
    #[serde(alias = "debt_mf_post_2023")]
    DebtMf,
    /// Unlisted bonds and debentures. Under Section 50AA, transfers on or after
    /// 23 Jul 2024 are short-term at slab rate whatever the holding period.
    UnlistedDebenturePost2024,
    /// Unlisted shares, property and other capital assets.
    OtherCapitalAsset,
    /// Physical gold and Sovereign Gold Bonds sold before maturity.
    GoldSgb,
}

/// Character of capital gain based on asset classification and holding period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldingPeriodCategory {
    ShortTerm,
    LongTerm,
}

/// The section of the Income-tax Act 1961 a disposal is taxed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndianTaxSection {
    /// Short-term gain on STT-paid equity, equity funds and business trust units.
    Sec111a,
    /// Long-term gain on STT-paid equity, equity funds and business trust units.
    Sec112a,
    /// Long-term gain on other capital assets.
    Sec112,
    /// Deemed short-term gain under Section 50AA, taxed at slab rate.
    Sec50aa,
    /// Other short-term gain, taxed at slab rate.
    SlabStcg,
    /// Long-term gain on listed equity sold before 1 Apr 2018, exempt under
    /// Section 10(38). Losses on such sales are not allowable either.
    Sec10_38,
}

/// 23 Jul 2024: Finance (No. 2) Act 2024 rates and holding periods take effect.
fn budget_2024_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 7, 23).expect("hard-coded date is valid")
}

/// 31 Jan 2018: Section 55(2)(ac) grandfathering cut-off.
fn grandfathering_cutoff() -> NaiveDate {
    NaiveDate::from_ymd_opt(2018, 1, 31).expect("hard-coded date is valid")
}

/// 1 Apr 2018: Section 112A replaces the Section 10(38) exemption.
fn sec_112a_start() -> NaiveDate {
    NaiveDate::from_ymd_opt(2018, 4, 1).expect("hard-coded date is valid")
}

/// 1 Apr 2023: Section 50AA starts applying to non-equity mutual funds.
fn sec_50aa_mf_start() -> NaiveDate {
    NaiveDate::from_ymd_opt(2023, 4, 1).expect("hard-coded date is valid")
}

/// Financial year (start year) that contains `date`: April to March.
pub fn financial_year_of(date: NaiveDate) -> i32 {
    use chrono::Datelike;
    if date.month() >= 4 {
        date.year()
    } else {
        date.year() - 1
    }
}

/// Section 112A exemption for a financial year (start year).
///
/// ₹1,25,000 from FY 2024-25 (it applies to the whole year, including sales
/// before 23 Jul 2024); ₹1,00,000 for FY 2018-19 to FY 2023-24; none before.
pub fn sec_112a_exemption_for_fy(fy_start_year: i32) -> Decimal {
    if fy_start_year >= 2024 {
        Decimal::new(125_000, 0)
    } else if fy_start_year >= 2018 {
        Decimal::new(100_000, 0)
    } else {
        Decimal::ZERO
    }
}

/// Cost Inflation Index (Section 48, Explanation (v)) by financial year
/// start, from the 2001-02 base year.
const COST_INFLATION_INDEX: [(i32, i64); 25] = [
    (2001, 100),
    (2002, 105),
    (2003, 109),
    (2004, 113),
    (2005, 117),
    (2006, 122),
    (2007, 129),
    (2008, 137),
    (2009, 148),
    (2010, 167),
    (2011, 184),
    (2012, 200),
    (2013, 220),
    (2014, 240),
    (2015, 254),
    (2016, 264),
    (2017, 272),
    (2018, 280),
    (2019, 289),
    (2020, 301),
    (2021, 317),
    (2022, 331),
    (2023, 348),
    (2024, 363),
    (2025, 376),
];

/// Cost Inflation Index for a financial year. Years before 2001-02 use the
/// base year, as cost is then taken from 1 Apr 2001 (Section 55(2)(b)).
/// `None` for a year not yet notified.
pub fn cost_inflation_index(fy_start_year: i32) -> Option<Decimal> {
    let year = fy_start_year.max(2001);
    COST_INFLATION_INDEX
        .iter()
        .find(|(fy, _)| *fy == year)
        .map(|(_, cii)| Decimal::new(*cii, 0))
}

/// Indexed cost of acquisition: cost × CII(year of sale) / CII(year of
/// purchase), rounded to the rupee-paisa. `None` when either index is
/// unknown.
pub fn indexed_cost(
    cost: Decimal,
    acquisition_date: NaiveDate,
    disposal_date: NaiveDate,
) -> Option<Decimal> {
    let bought = cost_inflation_index(financial_year_of(acquisition_date))?;
    let sold = cost_inflation_index(financial_year_of(disposal_date))?;
    Some((cost * sold / bought).round_dp(2))
}

/// Special tax rate for a section on a sale date. `None` means slab rate or exempt.
pub fn special_rate(section: IndianTaxSection, disposal_date: NaiveDate) -> Option<Decimal> {
    let post_budget = disposal_date >= budget_2024_date();
    match section {
        IndianTaxSection::Sec111a => Some(if post_budget {
            Decimal::new(20, 2)
        } else {
            Decimal::new(15, 2)
        }),
        IndianTaxSection::Sec112a => Some(if post_budget {
            Decimal::new(125, 3)
        } else {
            Decimal::new(10, 2)
        }),
        // Before 23 Jul 2024 this is 20% on the indexed gain.
        IndianTaxSection::Sec112 => Some(if post_budget {
            Decimal::new(125, 3)
        } else {
            Decimal::new(20, 2)
        }),
        IndianTaxSection::Sec50aa | IndianTaxSection::SlabStcg | IndianTaxSection::Sec10_38 => None,
    }
}

/// Calculates deemed cost of acquisition under Section 55(2)(ac) for STT-paid
/// equity acquired on or before 31 Jan 2018.
///
/// Deemed cost = max(actual cost, min(FMV on 31 Jan 2018, sale value)), with
/// every value for the same number of units. Returns `actual_cost` when the
/// lot was acquired after the cut-off or no FMV is known.
pub fn calculate_grandfathered_cost_basis(
    acquisition_date: NaiveDate,
    actual_cost: Decimal,
    fmv_2018_01_31_total: Option<Decimal>,
    sale_value: Decimal,
) -> Decimal {
    if acquisition_date > grandfathering_cutoff() {
        return actual_cost;
    }
    match fmv_2018_01_31_total {
        Some(fmv) => actual_cost.max(fmv.min(sale_value)),
        None => actual_cost,
    }
}

/// True when the asset was held for *more than* `months` months: the sale
/// falls after the same calendar date `months` later.
fn held_more_than(acquisition_date: NaiveDate, disposal_date: NaiveDate, months: u32) -> bool {
    match acquisition_date.checked_add_months(Months::new(months)) {
        Some(anniversary) => disposal_date > anniversary,
        None => false,
    }
}

fn is_equity_like(classification: IndianAssetTaxClassification) -> bool {
    matches!(
        classification,
        IndianAssetTaxClassification::EquityListed
            | IndianAssetTaxClassification::EquityMf
            | IndianAssetTaxClassification::ReitInvIt
    )
}

/// Whether Section 50AA deems this lot's gain short-term.
fn is_sec_50aa(
    acquisition_date: NaiveDate,
    disposal_date: NaiveDate,
    classification: IndianAssetTaxClassification,
) -> bool {
    match classification {
        IndianAssetTaxClassification::DebtMf => acquisition_date >= sec_50aa_mf_start(),
        IndianAssetTaxClassification::UnlistedDebenturePost2024 => {
            disposal_date >= budget_2024_date()
        }
        _ => false,
    }
}

/// Determines whether a gain is short-term or long-term (Section 2(42A) and 50AA).
pub fn determine_holding_period_category(
    acquisition_date: NaiveDate,
    disposal_date: NaiveDate,
    classification: IndianAssetTaxClassification,
) -> HoldingPeriodCategory {
    if is_sec_50aa(acquisition_date, disposal_date, classification) {
        return HoldingPeriodCategory::ShortTerm;
    }

    let post_budget = disposal_date >= budget_2024_date();
    let months = match classification {
        IndianAssetTaxClassification::EquityListed | IndianAssetTaxClassification::EquityMf => 12,
        IndianAssetTaxClassification::ReitInvIt => {
            if post_budget {
                12
            } else {
                36
            }
        }
        IndianAssetTaxClassification::DebtMf
        | IndianAssetTaxClassification::UnlistedDebenturePost2024
        | IndianAssetTaxClassification::OtherCapitalAsset
        | IndianAssetTaxClassification::GoldSgb => {
            if post_budget {
                24
            } else {
                36
            }
        }
    };

    if held_more_than(acquisition_date, disposal_date, months) {
        HoldingPeriodCategory::LongTerm
    } else {
        HoldingPeriodCategory::ShortTerm
    }
}

/// The tax section for one disposal.
pub fn determine_tax_section(
    acquisition_date: NaiveDate,
    disposal_date: NaiveDate,
    classification: IndianAssetTaxClassification,
) -> IndianTaxSection {
    if is_sec_50aa(acquisition_date, disposal_date, classification) {
        return IndianTaxSection::Sec50aa;
    }
    let holding =
        determine_holding_period_category(acquisition_date, disposal_date, classification);
    match (is_equity_like(classification), holding) {
        (true, HoldingPeriodCategory::ShortTerm) => IndianTaxSection::Sec111a,
        (true, HoldingPeriodCategory::LongTerm) => {
            if disposal_date < sec_112a_start() {
                IndianTaxSection::Sec10_38
            } else {
                IndianTaxSection::Sec112a
            }
        }
        (false, HoldingPeriodCategory::ShortTerm) => IndianTaxSection::SlabStcg,
        (false, HoldingPeriodCategory::LongTerm) => IndianTaxSection::Sec112,
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
    pub tax_section: IndianTaxSection,
    pub acquisition_date: NaiveDate,
    pub disposal_date: NaiveDate,
    pub holding_period: HoldingPeriodCategory,
    pub actual_cost: Decimal,
    pub deemed_cost: Decimal,
    pub proceeds: Decimal,
    pub gross_gain: Decimal,
    pub taxable_gain: Decimal,
    /// Special rate for this row's section, or 0 for slab-rate and exempt rows.
    pub applicable_tax_rate: Decimal,
    /// Indicative tax on this row alone: gain × special rate, before loss
    /// set-off and the Section 112A exemption. Year totals are in the summary.
    pub estimated_tax: Decimal,
    // ── Fields for Schedule CG / ITR export ──
    pub asset_name: String,
    pub asset_symbol: Option<String>,
    pub isin: Option<String>,
    pub quantity: Decimal,
    pub sale_price_per_unit: Decimal,
    pub cost_per_unit: Decimal,
    pub fmv_per_unit_2018: Option<Decimal>,
    pub transfer_expenses: Decimal,
}

/// Gains taxed at one special rate, after loss set-off and the Section 112A
/// exemption. Used for surcharge and the basic exemption adjustment, which
/// depend on the person's other income.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianSpecialRateGain {
    pub section: IndianTaxSection,
    pub rate: Decimal,
    pub amount: Decimal,
}

/// Trade charges behind one disposal, in base currency, for the units it
/// disposed of. Section 48 allows the expenses of the transfer (brokerage,
/// stamp duty, exchange charges) as a deduction, but not securities
/// transaction tax, so a trade's `fee` is the transfer expense and its `tax`
/// (STT) is added back to the sale value and taken off the cost.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IndianDisposalCharges {
    /// The sell trade's fee share.
    pub sale_fee: Decimal,
    /// The sell trade's tax (STT) share.
    pub sale_tax: Decimal,
    /// The buy trade's tax (STT) share, which the lot's cost includes.
    pub purchase_tax: Decimal,
}

/// Compact asset info used by the Indian tax calculator for export fields.
pub struct IndianTaxAssetInfo {
    pub name: String,
    pub symbol: Option<String>,
    pub isin: Option<String>,
}

/// Annual Indian capital gains summary for one financial year.
///
/// Gross fields are before loss set-off. `sec_112a_*_taxable`, the estimates
/// and the carry-forward fields are after Section 70 set-off.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianTaxYearSummary {
    /// Financial year start (2025 = FY 2025-26).
    pub fy_start_year: i32,
    /// Section 111A STCG on sales before 23 Jul 2024 (15%), gross
    pub stcg_111a_pre_budget: Decimal,
    /// Section 111A STCG on sales from 23 Jul 2024 (20%), gross
    pub stcg_111a_post_budget: Decimal,
    /// STCG at slab rate (Section 50AA and other short-term gains), gross
    pub stcg_slab_total: Decimal,
    /// Slab-rate STCG left after loss set-off: add this to taxable income
    pub stcg_slab_net: Decimal,
    /// Total STCG before loss set-off
    pub stcg_total: Decimal,
    /// Section 112A LTCG on sales before 23 Jul 2024 (10%), gross
    pub ltcg_112a_pre_budget: Decimal,
    /// Section 112A LTCG on sales from 23 Jul 2024 (12.5%), gross
    pub ltcg_112a_post_budget: Decimal,
    /// Total Section 112A LTCG before loss set-off
    pub sec_112a_ltcg_gross: Decimal,
    /// Section 112A exemption used (after set-off)
    pub sec_112a_exemption_applied: Decimal,
    /// Section 112A LTCG taxable after set-off and exemption
    pub sec_112a_ltcg_taxable: Decimal,
    /// Section 112 LTCG on other assets, gross
    pub ltcg_other_total: Decimal,
    /// Total LTCG before loss set-off
    pub ltcg_total: Decimal,
    /// Long-term gain on listed equity sold before 1 Apr 2018, exempt under Section 10(38)
    pub exempt_ltcg_sec_10_38: Decimal,
    /// Short-term capital loss left to carry forward (Section 74)
    pub stcl_carried_forward: Decimal,
    /// Long-term capital loss left to carry forward (Section 74)
    pub ltcl_carried_forward: Decimal,
    /// Tax on special-rate STCG (Section 111A) after set-off
    pub estimated_stcg_tax: Decimal,
    /// Tax on special-rate LTCG (Sections 112A and 112) after set-off and exemption
    pub estimated_ltcg_tax: Decimal,
    /// 4% health and education cess on the two estimates above
    pub health_education_cess: Decimal,
    /// Estimated STCG + LTCG tax + cess. Excludes slab-rate gains and surcharge.
    pub estimated_total_tax: Decimal,
    /// Special-rate gains by section and rate, after set-off and exemption.
    /// Only rates with a positive amount are listed.
    pub special_rate_gains: Vec<IndianSpecialRateGain>,
    /// Things the person should check before filing
    pub warnings: Vec<String>,
    /// ClearTax workbooks the year needs (5,000 rows per sheet per file)
    pub cleartax_files: usize,
    pub calculations: Vec<IndianCapitalGainCalculation>,
}

/// Rate buckets for Section 70 set-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Bucket {
    Stcg111aPre,
    Stcg111aPost,
    StcgSlab,
    Ltcg112aPre,
    Ltcg112aPost,
    Ltcg112Pre,
    /// Listed securities other than units sold before 23 Jul 2024, where 10%
    /// without indexation is lower than 20% with it (Section 112(1) proviso).
    Ltcg112PreProviso,
    Ltcg112Post,
}

/// Short-term gains, highest rate first (slab is assumed to be the highest).
const ST_ORDER: [Bucket; 3] = [Bucket::StcgSlab, Bucket::Stcg111aPost, Bucket::Stcg111aPre];

/// Long-term gains, highest rate first; at equal rates Section 112 goes before
/// 112A, because 112A gains may fall inside the exemption anyway.
const LT_ORDER: [Bucket; 5] = [
    Bucket::Ltcg112Pre,
    Bucket::Ltcg112Post,
    Bucket::Ltcg112aPost,
    Bucket::Ltcg112PreProviso,
    Bucket::Ltcg112aPre,
];

fn bucket_for(section: IndianTaxSection, disposal_date: NaiveDate) -> Option<Bucket> {
    let post = disposal_date >= budget_2024_date();
    match section {
        IndianTaxSection::Sec111a if post => Some(Bucket::Stcg111aPost),
        IndianTaxSection::Sec111a => Some(Bucket::Stcg111aPre),
        IndianTaxSection::Sec50aa | IndianTaxSection::SlabStcg => Some(Bucket::StcgSlab),
        IndianTaxSection::Sec112a if post => Some(Bucket::Ltcg112aPost),
        IndianTaxSection::Sec112a => Some(Bucket::Ltcg112aPre),
        IndianTaxSection::Sec112 if post => Some(Bucket::Ltcg112Post),
        IndianTaxSection::Sec112 => Some(Bucket::Ltcg112Pre),
        IndianTaxSection::Sec10_38 => None,
    }
}

/// Uses `loss` against the positive buckets in `order`, returning what is left.
fn absorb(loss: Decimal, order: &[Bucket], nets: &mut HashMap<Bucket, Decimal>) -> Decimal {
    let mut left = loss;
    for bucket in order {
        if left <= Decimal::ZERO {
            break;
        }
        let gain = nets.entry(*bucket).or_insert(Decimal::ZERO);
        if *gain > Decimal::ZERO {
            let used = left.min(*gain);
            *gain -= used;
            left -= used;
        }
    }
    left
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.get(..10)?, "%Y-%m-%d").ok()
}

/// Calculates Indian capital gains for one financial year of lot disposals.
///
/// `asset_fmv_2018_per_unit` holds the 31 Jan 2018 fair market value per
/// unit (highest listed price that day for shares, NAV for funds), as the ITR
/// and ClearTax ask for it.
pub fn calculate_indian_capital_gains(
    disposals: &[LotDisposal],
    lot_open_dates: &HashMap<String, NaiveDate>,
    asset_classifications: &HashMap<String, IndianAssetTaxClassification>,
    asset_fmv_2018_per_unit: &HashMap<String, Decimal>,
    asset_info: &HashMap<String, IndianTaxAssetInfo>,
    charges: &HashMap<String, IndianDisposalCharges>,
    fy_start_year: i32,
) -> IndianTaxYearSummary {
    let mut summary = IndianTaxYearSummary {
        fy_start_year,
        ..Default::default()
    };
    let mut nets: HashMap<Bucket, Decimal> = HashMap::new();
    let mut missing_open_dates = 0usize;
    let mut non_fifo = false;
    let mut missing_fmv: std::collections::BTreeSet<String> = Default::default();
    let mut unindexed = false;

    for d in disposals {
        let Some(disp_date) = parse_date(&d.disposal_date) else {
            continue;
        };
        if !d.cost_basis_method.eq_ignore_ascii_case("FIFO") {
            non_fifo = true;
        }
        let acq_date = match lot_open_dates.get(&d.lot_id) {
            Some(date) => *date,
            None => {
                missing_open_dates += 1;
                disp_date
            }
        };

        let classification = asset_classifications
            .get(&d.asset_id)
            .copied()
            .unwrap_or(IndianAssetTaxClassification::OtherCapitalAsset);

        // Lot cost and sale proceeds are net of the trades' fee and tax. Put
        // STT back: it is neither cost nor transfer expense.
        let charge = charges.get(&d.id).copied().unwrap_or_default();
        let cost_basis: Decimal =
            d.cost_basis_base.parse().unwrap_or(Decimal::ZERO) - charge.purchase_tax;
        let proceeds: Decimal =
            d.proceeds_base.parse().unwrap_or(Decimal::ZERO) + charge.sale_fee + charge.sale_tax;
        let transfer_expenses = charge.sale_fee;
        let qty: Decimal = d.quantity.parse().unwrap_or(Decimal::ZERO);
        let (sale_price_per_unit, cost_per_unit) = if qty > Decimal::ZERO {
            (proceeds / qty, cost_basis / qty)
        } else {
            (Decimal::ZERO, Decimal::ZERO)
        };
        let fmv_per_unit_2018 = asset_fmv_2018_per_unit.get(&d.asset_id).copied();

        let info = asset_info.get(&d.asset_id);
        let holding_period = determine_holding_period_category(acq_date, disp_date, classification);
        let tax_section = determine_tax_section(acq_date, disp_date, classification);

        if tax_section == IndianTaxSection::Sec112a
            && acq_date <= grandfathering_cutoff()
            && fmv_per_unit_2018.is_none()
        {
            missing_fmv.insert(
                info.map(|i| i.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| d.asset_id.clone()),
            );
        }

        let deemed_cost = if tax_section == IndianTaxSection::Sec112a {
            calculate_grandfathered_cost_basis(
                acq_date,
                cost_basis,
                fmv_per_unit_2018.map(|fmv| fmv * qty),
                proceeds,
            )
        } else {
            cost_basis
        };
        // Section 112 before 23 Jul 2024: 20% on the indexed gain. For a
        // listed security that isn't a unit (here, SGBs), 10% on the gain
        // without indexation applies when lower.
        let mut deemed_cost = deemed_cost;
        let mut rate = special_rate(tax_section, disp_date);
        let mut bucket = bucket_for(tax_section, disp_date);
        if tax_section == IndianTaxSection::Sec112 && disp_date < budget_2024_date() {
            match indexed_cost(cost_basis, acq_date, disp_date) {
                Some(indexed) => {
                    let net = proceeds - transfer_expenses;
                    let indexed_tax = (net - indexed).max(Decimal::ZERO) * Decimal::new(20, 2);
                    let plain_tax = (net - cost_basis).max(Decimal::ZERO) * Decimal::new(10, 2);
                    if classification == IndianAssetTaxClassification::GoldSgb
                        && plain_tax < indexed_tax
                    {
                        rate = Some(Decimal::new(10, 2));
                        bucket = Some(Bucket::Ltcg112PreProviso);
                    } else {
                        deemed_cost = indexed;
                    }
                }
                None => unindexed = true,
            }
        }
        let gross_gain = proceeds - deemed_cost - transfer_expenses;

        match tax_section {
            IndianTaxSection::Sec111a => {
                if disp_date < budget_2024_date() {
                    summary.stcg_111a_pre_budget += gross_gain;
                } else {
                    summary.stcg_111a_post_budget += gross_gain;
                }
                summary.stcg_total += gross_gain;
            }
            IndianTaxSection::Sec50aa | IndianTaxSection::SlabStcg => {
                summary.stcg_slab_total += gross_gain;
                summary.stcg_total += gross_gain;
            }
            IndianTaxSection::Sec112a => {
                if disp_date < budget_2024_date() {
                    summary.ltcg_112a_pre_budget += gross_gain;
                } else {
                    summary.ltcg_112a_post_budget += gross_gain;
                }
                summary.sec_112a_ltcg_gross += gross_gain;
                summary.ltcg_total += gross_gain;
            }
            IndianTaxSection::Sec112 => {
                summary.ltcg_other_total += gross_gain;
                summary.ltcg_total += gross_gain;
            }
            IndianTaxSection::Sec10_38 => {
                summary.exempt_ltcg_sec_10_38 += gross_gain;
            }
        }
        if let Some(bucket) = bucket {
            *nets.entry(bucket).or_insert(Decimal::ZERO) += gross_gain;
        }

        summary.calculations.push(IndianCapitalGainCalculation {
            disposal_id: d.id.clone(),
            lot_id: d.lot_id.clone(),
            asset_id: d.asset_id.clone(),
            classification,
            tax_section,
            acquisition_date: acq_date,
            disposal_date: disp_date,
            holding_period,
            actual_cost: cost_basis,
            deemed_cost,
            proceeds,
            gross_gain,
            taxable_gain: gross_gain,
            applicable_tax_rate: rate.unwrap_or(Decimal::ZERO),
            estimated_tax: rate
                .map(|r| gross_gain.max(Decimal::ZERO) * r)
                .unwrap_or(Decimal::ZERO),
            asset_name: info.map(|i| i.name.clone()).unwrap_or_default(),
            asset_symbol: info.and_then(|i| i.symbol.clone()),
            isin: info.and_then(|i| i.isin.clone()),
            quantity: qty,
            sale_price_per_unit,
            cost_per_unit,
            fmv_per_unit_2018,
            transfer_expenses,
        });
    }

    // ── Section 70 set-off ──
    // Pull the losses out of each bucket. Long-term losses can only go against
    // long-term gains, so they are placed first; short-term losses then go
    // against short-term gains and whatever long-term gain is left.
    let mut stcl = Decimal::ZERO;
    let mut ltcl = Decimal::ZERO;
    for (bucket, value) in nets.iter_mut() {
        if *value < Decimal::ZERO {
            if ST_ORDER.contains(bucket) {
                stcl += -*value;
            } else {
                ltcl += -*value;
            }
            *value = Decimal::ZERO;
        }
    }
    let ltcl_left = absorb(ltcl, &LT_ORDER, &mut nets);
    let stcl_after_st = absorb(stcl, &ST_ORDER, &mut nets);
    let stcl_left = absorb(stcl_after_st, &LT_ORDER, &mut nets);

    // ── Section 112A exemption, after set-off; the 12.5% pool first ──
    let get =
        |nets: &HashMap<Bucket, Decimal>, b: Bucket| nets.get(&b).copied().unwrap_or_default();
    let mut ltcg_112a_post = get(&nets, Bucket::Ltcg112aPost);
    let mut ltcg_112a_pre = get(&nets, Bucket::Ltcg112aPre);
    let exemption_limit = sec_112a_exemption_for_fy(fy_start_year);
    let mut exemption_left = exemption_limit;
    for pool in [&mut ltcg_112a_post, &mut ltcg_112a_pre] {
        let used = exemption_left.min(*pool);
        *pool -= used;
        exemption_left -= used;
    }
    summary.sec_112a_exemption_applied = exemption_limit - exemption_left;
    summary.sec_112a_ltcg_taxable = ltcg_112a_post + ltcg_112a_pre;

    // ── Estimates ──
    summary.stcg_slab_net = get(&nets, Bucket::StcgSlab);
    summary.estimated_stcg_tax = get(&nets, Bucket::Stcg111aPre) * Decimal::new(15, 2)
        + get(&nets, Bucket::Stcg111aPost) * Decimal::new(20, 2);
    summary.estimated_ltcg_tax = ltcg_112a_pre * Decimal::new(10, 2)
        + ltcg_112a_post * Decimal::new(125, 3)
        + get(&nets, Bucket::Ltcg112Pre) * Decimal::new(20, 2)
        + get(&nets, Bucket::Ltcg112PreProviso) * Decimal::new(10, 2)
        + get(&nets, Bucket::Ltcg112Post) * Decimal::new(125, 3);
    use IndianTaxSection::{Sec111a, Sec112, Sec112a};
    summary.special_rate_gains = [
        (
            Sec111a,
            Decimal::new(15, 2),
            get(&nets, Bucket::Stcg111aPre),
        ),
        (
            Sec111a,
            Decimal::new(20, 2),
            get(&nets, Bucket::Stcg111aPost),
        ),
        (Sec112a, Decimal::new(10, 2), ltcg_112a_pre),
        (Sec112a, Decimal::new(125, 3), ltcg_112a_post),
        (Sec112, Decimal::new(20, 2), get(&nets, Bucket::Ltcg112Pre)),
        (
            Sec112,
            Decimal::new(10, 2),
            get(&nets, Bucket::Ltcg112PreProviso),
        ),
        (
            Sec112,
            Decimal::new(125, 3),
            get(&nets, Bucket::Ltcg112Post),
        ),
    ]
    .into_iter()
    .filter(|(_, _, amount)| *amount > Decimal::ZERO)
    .map(|(section, rate, amount)| IndianSpecialRateGain {
        section,
        rate,
        amount,
    })
    .collect();
    let special_tax = summary.estimated_stcg_tax + summary.estimated_ltcg_tax;
    summary.health_education_cess = special_tax * Decimal::new(4, 2);
    summary.estimated_total_tax = special_tax + summary.health_education_cess;

    // ── Section 74 carry-forward ──
    summary.stcl_carried_forward = stcl_left;
    summary.ltcl_carried_forward = ltcl_left;

    if non_fifo {
        summary.warnings.push(
            "Some sales use a cost basis method other than FIFO. Indian tax law \
             (Section 45(2A)) requires FIFO for dematerialised securities."
                .to_string(),
        );
    }
    if missing_open_dates > 0 {
        summary.warnings.push(format!(
            "{} sale(s) have no purchase date and are treated as short-term.",
            missing_open_dates
        ));
    }
    if !missing_fmv.is_empty() {
        summary.warnings.push(format!(
            "No 31 Jan 2018 price is set for {}. These were bought before Feb 2018, so \
             grandfathering is skipped and the gain may be overstated. Set the price on \
             each asset's page.",
            missing_fmv.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if unindexed {
        summary.warnings.push(
            "Some Section 112 sales before 23 Jul 2024 fall in a year with no Cost \
             Inflation Index here, so they are estimated at 20% without indexation and \
             the estimate may be high."
                .to_string(),
        );
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// One sale of one lot, for building worked examples.
    struct Sale {
        asset: &'static str,
        class: IndianAssetTaxClassification,
        bought: NaiveDate,
        sold: NaiveDate,
        qty: Decimal,
        cost: Decimal,
        proceeds: Decimal,
        fmv_per_unit: Option<Decimal>,
    }

    fn sale(
        asset: &'static str,
        class: IndianAssetTaxClassification,
        bought: NaiveDate,
        sold: NaiveDate,
        cost: Decimal,
        proceeds: Decimal,
    ) -> Sale {
        Sale {
            asset,
            class,
            bought,
            sold,
            qty: dec!(1),
            cost,
            proceeds,
            fmv_per_unit: None,
        }
    }

    fn run(sales: &[Sale], fy: i32) -> IndianTaxYearSummary {
        run_with_charges(sales, &HashMap::new(), fy)
    }

    fn run_with_charges(
        sales: &[Sale],
        charges: &HashMap<String, IndianDisposalCharges>,
        fy: i32,
    ) -> IndianTaxYearSummary {
        let mut disposals = Vec::new();
        let mut open_dates = HashMap::new();
        let mut classes = HashMap::new();
        let mut fmvs = HashMap::new();
        for (i, s) in sales.iter().enumerate() {
            let lot_id = format!("lot-{i}");
            open_dates.insert(lot_id.clone(), s.bought);
            classes.insert(s.asset.to_string(), s.class);
            if let Some(fmv) = s.fmv_per_unit {
                fmvs.insert(s.asset.to_string(), fmv);
            }
            disposals.push(LotDisposal {
                id: format!("disp-{i}"),
                lot_id,
                account_id: "acc".to_string(),
                asset_id: s.asset.to_string(),
                disposal_activity_id: format!("act-{i}"),
                disposal_date: s.sold.format("%Y-%m-%d").to_string(),
                quantity: s.qty.to_string(),
                proceeds: s.proceeds.to_string(),
                cost_basis: s.cost.to_string(),
                realized_pnl: (s.proceeds - s.cost).to_string(),
                proceeds_base: s.proceeds.to_string(),
                cost_basis_base: s.cost.to_string(),
                realized_pnl_base: (s.proceeds - s.cost).to_string(),
                currency: "INR".to_string(),
                base_currency: "INR".to_string(),
                fx_rate_to_base: "1".to_string(),
                cost_basis_method: "FIFO".to_string(),
                created_at: "2025-01-01T00:00:00.000Z".to_string(),
            });
        }
        calculate_indian_capital_gains(
            &disposals,
            &open_dates,
            &classes,
            &fmvs,
            &HashMap::new(),
            charges,
            fy,
        )
    }

    use IndianAssetTaxClassification::*;

    // ── Rates and limits ──

    #[test]
    fn rates_switch_on_23_jul_2024() {
        let pre = date(2024, 7, 22);
        let post = date(2024, 7, 23);
        assert_eq!(
            special_rate(IndianTaxSection::Sec111a, pre),
            Some(dec!(0.15))
        );
        assert_eq!(
            special_rate(IndianTaxSection::Sec111a, post),
            Some(dec!(0.20))
        );
        assert_eq!(
            special_rate(IndianTaxSection::Sec112a, pre),
            Some(dec!(0.10))
        );
        assert_eq!(
            special_rate(IndianTaxSection::Sec112a, post),
            Some(dec!(0.125))
        );
        assert_eq!(
            special_rate(IndianTaxSection::Sec112, post),
            Some(dec!(0.125))
        );
        assert_eq!(special_rate(IndianTaxSection::Sec50aa, post), None);
    }

    #[test]
    fn exemption_is_per_financial_year() {
        assert_eq!(sec_112a_exemption_for_fy(2017), dec!(0));
        assert_eq!(sec_112a_exemption_for_fy(2023), dec!(100000));
        // The ₹1.25L limit covers all of FY 2024-25, including April to July.
        assert_eq!(sec_112a_exemption_for_fy(2024), dec!(125000));
        assert_eq!(financial_year_of(date(2025, 3, 31)), 2024);
        assert_eq!(financial_year_of(date(2025, 4, 1)), 2025);
    }

    // ── Holding periods ──

    #[test]
    fn sale_on_the_one_year_anniversary_is_short_term() {
        // 15 Jan 2024 to 15 Jan 2025 is 366 days across 29 Feb, but not *more*
        // than 12 months.
        assert_eq!(
            determine_holding_period_category(date(2024, 1, 15), date(2025, 1, 15), EquityListed),
            HoldingPeriodCategory::ShortTerm
        );
        assert_eq!(
            determine_holding_period_category(date(2024, 1, 15), date(2025, 1, 16), EquityListed),
            HoldingPeriodCategory::LongTerm
        );
    }

    #[test]
    fn holding_periods_follow_the_sale_date() {
        let bought = date(2022, 1, 1);
        // About 26 months in both cases.
        let before = date(2024, 3, 11);
        let after = date(2024, 8, 1);
        assert_eq!(
            determine_holding_period_category(bought, before, ReitInvIt),
            HoldingPeriodCategory::ShortTerm
        );
        assert_eq!(
            determine_holding_period_category(bought, after, ReitInvIt),
            HoldingPeriodCategory::LongTerm
        );
        assert_eq!(
            determine_holding_period_category(bought, before, GoldSgb),
            HoldingPeriodCategory::ShortTerm
        );
        assert_eq!(
            determine_holding_period_category(bought, after, GoldSgb),
            HoldingPeriodCategory::LongTerm
        );
    }

    #[test]
    fn debt_fund_50aa_depends_on_each_lots_purchase_date() {
        let sold = date(2025, 9, 1);
        // Bought 31 Mar 2023: old rules, more than 24 months → long-term, Section 112.
        assert_eq!(
            determine_tax_section(date(2023, 3, 31), sold, DebtMf),
            IndianTaxSection::Sec112
        );
        // Bought 1 Apr 2023: Section 50AA, short-term at slab rate whatever the period.
        assert_eq!(
            determine_tax_section(date(2023, 4, 1), sold, DebtMf),
            IndianTaxSection::Sec50aa
        );
    }

    #[test]
    fn unlisted_debentures_follow_50aa_only_from_23_jul_2024() {
        let bought = date(2019, 1, 1);
        assert_eq!(
            determine_tax_section(bought, date(2024, 7, 22), UnlistedDebenturePost2024),
            IndianTaxSection::Sec112
        );
        assert_eq!(
            determine_tax_section(bought, date(2024, 7, 23), UnlistedDebenturePost2024),
            IndianTaxSection::Sec50aa
        );
    }

    #[test]
    fn legacy_debt_mf_metadata_name_still_parses() {
        let parsed: IndianAssetTaxClassification =
            serde_json::from_str("\"debt_mf_post_2023\"").unwrap();
        assert_eq!(parsed, DebtMf);
    }

    // ── Grandfathering ──

    #[test]
    fn grandfathering_formula_cases() {
        let pre = date(2017, 5, 10);
        let post = date(2018, 2, 1);
        assert_eq!(
            calculate_grandfathered_cost_basis(post, dec!(100), Some(dec!(150)), dec!(200)),
            dec!(100)
        );
        assert_eq!(
            calculate_grandfathered_cost_basis(pre, dec!(100), Some(dec!(150)), dec!(200)),
            dec!(150)
        );
        assert_eq!(
            calculate_grandfathered_cost_basis(pre, dec!(100), Some(dec!(150)), dec!(120)),
            dec!(120)
        );
        assert_eq!(
            calculate_grandfathered_cost_basis(pre, dec!(100), Some(dec!(150)), dec!(80)),
            dec!(100)
        );
        assert_eq!(
            calculate_grandfathered_cost_basis(pre, dec!(100), Some(dec!(80)), dec!(200)),
            dec!(100)
        );
    }

    #[test]
    fn grandfathering_uses_fmv_per_unit_times_quantity() {
        // 100 shares bought 2017 at ₹1,000; FMV ₹1,500 per share; sold at ₹2,000.
        // Deemed cost = max(1,00,000, min(1,50,000, 2,00,000)) = 1,50,000 → gain 50,000.
        let summary = run(
            &[Sale {
                asset: "RELIANCE",
                class: EquityListed,
                bought: date(2017, 6, 15),
                sold: date(2024, 8, 10),
                qty: dec!(100),
                cost: dec!(100000),
                proceeds: dec!(200000),
                fmv_per_unit: Some(dec!(1500)),
            }],
            2024,
        );
        let row = &summary.calculations[0];
        assert_eq!(row.deemed_cost, dec!(150000));
        assert_eq!(row.gross_gain, dec!(50000));
        assert_eq!(row.tax_section, IndianTaxSection::Sec112a);
    }

    #[test]
    fn missing_2018_price_on_old_holding_raises_a_warning() {
        let summary = run(
            &[
                sale(
                    "OLD",
                    EquityListed,
                    date(2017, 6, 15),
                    date(2024, 8, 10),
                    dec!(1000),
                    dec!(2000),
                ),
                // Bought after the cut-off: no FMV needed.
                sale(
                    "NEW",
                    EquityListed,
                    date(2019, 6, 15),
                    date(2024, 8, 10),
                    dec!(1000),
                    dec!(2000),
                ),
            ],
            2024,
        );
        let w: Vec<_> = summary
            .warnings
            .iter()
            .filter(|w| w.contains("31 Jan 2018"))
            .collect();
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("OLD") && !w[0].contains("NEW"));
    }

    // ── Worked years ──

    #[test]
    fn mixed_year_with_grandfathering_and_budget_split() {
        // FY 2024-25.
        // A: STCG ₹20,000 on 5 Aug 2024 → 20%.
        // B: grandfathered LTCG: 100 units, cost ₹1L, FMV ₹1,500/unit, sale ₹3L
        //    on 1 Aug 2024 → deemed cost ₹1.5L, gain ₹1.5L at 12.5%.
        // 112A: 1,50,000 − 1,25,000 exemption = 25,000 × 12.5% = 3,125.
        // 111A: 20,000 × 20% = 4,000. Cess 4% of 7,125 = 285. Total 7,410.
        let summary = run(
            &[
                sale(
                    "TCS",
                    EquityListed,
                    date(2024, 3, 1),
                    date(2024, 8, 5),
                    dec!(50000),
                    dec!(70000),
                ),
                Sale {
                    asset: "RELIANCE",
                    class: EquityListed,
                    bought: date(2017, 1, 1),
                    sold: date(2024, 8, 1),
                    qty: dec!(100),
                    cost: dec!(100000),
                    proceeds: dec!(300000),
                    fmv_per_unit: Some(dec!(1500)),
                },
            ],
            2024,
        );
        assert_eq!(summary.stcg_total, dec!(20000));
        assert_eq!(summary.estimated_stcg_tax, dec!(4000));
        assert_eq!(summary.sec_112a_ltcg_gross, dec!(150000));
        assert_eq!(summary.sec_112a_exemption_applied, dec!(125000));
        assert_eq!(summary.sec_112a_ltcg_taxable, dec!(25000));
        assert_eq!(summary.estimated_ltcg_tax, dec!(3125));
        assert_eq!(summary.health_education_cess, dec!(285));
        assert_eq!(summary.estimated_total_tax, dec!(7410));
    }

    #[test]
    fn losses_are_set_off_before_the_112a_exemption() {
        // 112A gain ₹2L, STCL ₹1L. Net 112A ₹1L is inside the ₹1.25L exemption:
        // no tax and nothing to carry forward.
        let summary = run(
            &[
                sale(
                    "INFY",
                    EquityListed,
                    date(2020, 1, 1),
                    date(2025, 6, 1),
                    dec!(100000),
                    dec!(300000),
                ),
                sale(
                    "WIPRO",
                    EquityListed,
                    date(2025, 3, 1),
                    date(2025, 6, 2),
                    dec!(200000),
                    dec!(100000),
                ),
            ],
            2025,
        );
        assert_eq!(summary.sec_112a_ltcg_gross, dec!(200000));
        assert_eq!(summary.sec_112a_exemption_applied, dec!(100000));
        assert_eq!(summary.sec_112a_ltcg_taxable, dec!(0));
        assert_eq!(summary.estimated_total_tax, dec!(0));
        assert_eq!(summary.stcl_carried_forward, dec!(0));
    }

    #[test]
    fn short_term_losses_net_within_111a() {
        // STCG 40,000 and STCL 10,000 in the same year: 30,000 × 20% = 6,000.
        let summary = run(
            &[
                sale(
                    "INFY",
                    EquityListed,
                    date(2024, 7, 1),
                    date(2024, 8, 10),
                    dec!(60000),
                    dec!(100000),
                ),
                sale(
                    "WIPRO",
                    EquityListed,
                    date(2024, 7, 1),
                    date(2024, 8, 12),
                    dec!(50000),
                    dec!(40000),
                ),
            ],
            2024,
        );
        assert_eq!(summary.stcg_total, dec!(30000));
        assert_eq!(summary.estimated_stcg_tax, dec!(6000));
        assert_eq!(summary.stcl_carried_forward, dec!(0));
    }

    #[test]
    fn exemption_goes_to_the_12_5_percent_pool_first() {
        // FY 2024-25: 112A gain ₹1L before 23 Jul (10%), ₹1L after (12.5%).
        // Exempting the 12.5% pool first leaves ₹75,000 at 10% = 7,500
        // (the other way round would be ₹75,000 at 12.5% = 9,375).
        let summary = run(
            &[
                sale(
                    "HDFC",
                    EquityListed,
                    date(2020, 1, 1),
                    date(2024, 5, 1),
                    dec!(100000),
                    dec!(200000),
                ),
                sale(
                    "ITC",
                    EquityListed,
                    date(2020, 1, 1),
                    date(2024, 9, 1),
                    dec!(100000),
                    dec!(200000),
                ),
            ],
            2024,
        );
        assert_eq!(summary.sec_112a_ltcg_taxable, dec!(75000));
        assert_eq!(summary.estimated_ltcg_tax, dec!(7500));
    }

    #[test]
    fn long_term_losses_cross_112_and_112a() {
        // 112A loss ₹50,000 against a Section 112 gain of ₹2L on a gold bond:
        // 1,50,000 × 12.5% = 18,750.
        let summary = run(
            &[
                sale(
                    "SGB",
                    GoldSgb,
                    date(2020, 1, 1),
                    date(2025, 6, 1),
                    dec!(100000),
                    dec!(300000),
                ),
                sale(
                    "PAYTM",
                    EquityListed,
                    date(2021, 11, 18),
                    date(2025, 6, 1),
                    dec!(150000),
                    dec!(100000),
                ),
            ],
            2025,
        );
        assert_eq!(summary.estimated_ltcg_tax, dec!(18750));
        assert_eq!(summary.ltcl_carried_forward, dec!(0));
    }

    #[test]
    fn long_term_loss_cannot_touch_short_term_gain() {
        // LTCL ₹30,000 with only STCG ₹50,000: STCG stays taxed, LTCL carries forward.
        let summary = run(
            &[
                sale(
                    "A",
                    EquityListed,
                    date(2025, 5, 1),
                    date(2025, 8, 1),
                    dec!(50000),
                    dec!(100000),
                ),
                sale(
                    "B",
                    EquityListed,
                    date(2020, 1, 1),
                    date(2025, 8, 1),
                    dec!(80000),
                    dec!(50000),
                ),
            ],
            2025,
        );
        assert_eq!(summary.estimated_stcg_tax, dec!(10000));
        assert_eq!(summary.ltcl_carried_forward, dec!(30000));
        assert_eq!(summary.stcl_carried_forward, dec!(0));
    }

    #[test]
    fn unabsorbed_short_term_loss_carries_forward() {
        let summary = run(
            &[sale(
                "A",
                EquityListed,
                date(2025, 5, 1),
                date(2025, 8, 1),
                dec!(100000),
                dec!(60000),
            )],
            2025,
        );
        assert_eq!(summary.stcl_carried_forward, dec!(40000));
        assert_eq!(summary.estimated_total_tax, dec!(0));
    }

    #[test]
    fn debt_fund_50aa_gain_is_reported_for_slab_not_taxed_here() {
        let summary = run(
            &[sale(
                "LIQUID",
                DebtMf,
                date(2023, 6, 1),
                date(2025, 8, 1),
                dec!(100000),
                dec!(115000),
            )],
            2025,
        );
        assert_eq!(
            summary.calculations[0].tax_section,
            IndianTaxSection::Sec50aa
        );
        assert_eq!(summary.stcg_slab_net, dec!(15000));
        assert_eq!(summary.estimated_total_tax, dec!(0));
    }

    #[test]
    fn equity_sold_before_april_2018_is_exempt_under_10_38() {
        let summary = run(
            &[sale(
                "SBIN",
                EquityListed,
                date(2015, 1, 1),
                date(2018, 3, 1),
                dec!(100000),
                dec!(150000),
            )],
            2017,
        );
        assert_eq!(
            summary.calculations[0].tax_section,
            IndianTaxSection::Sec10_38
        );
        assert_eq!(summary.exempt_ltcg_sec_10_38, dec!(50000));
        assert_eq!(summary.estimated_total_tax, dec!(0));
    }

    #[test]
    fn non_fifo_sales_raise_a_warning() {
        let mut disposals = Vec::new();
        let base = run(
            &[sale(
                "A",
                EquityListed,
                date(2025, 5, 1),
                date(2025, 8, 1),
                dec!(100),
                dec!(110),
            )],
            2025,
        );
        assert!(base.warnings.is_empty());
        disposals.push(LotDisposal {
            id: "d".into(),
            lot_id: "l".into(),
            account_id: "a".into(),
            asset_id: "A".into(),
            disposal_activity_id: "x".into(),
            disposal_date: "2025-08-01".into(),
            quantity: "1".into(),
            proceeds: "110".into(),
            cost_basis: "100".into(),
            realized_pnl: "10".into(),
            proceeds_base: "110".into(),
            cost_basis_base: "100".into(),
            realized_pnl_base: "10".into(),
            currency: "INR".into(),
            base_currency: "INR".into(),
            fx_rate_to_base: "1".into(),
            cost_basis_method: "LIFO".into(),
            created_at: "2025-08-01".into(),
        });
        let summary = calculate_indian_capital_gains(
            &disposals,
            &HashMap::from([("l".to_string(), date(2025, 5, 1))]),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            2025,
        );
        assert!(summary.warnings.iter().any(|w| w.contains("FIFO")));
    }

    // Bought for 1,00,000 + 100 STT, sold for 1,50,000 less 40 brokerage and
    // 150 STT. Cost and proceeds come in net of the charges.
    #[test]
    fn stt_is_not_deducted_and_brokerage_is_a_transfer_expense() {
        let s = sale(
            "eq",
            EquityListed,
            date(2024, 5, 1),
            date(2024, 12, 2),
            dec!(100100),
            dec!(149810),
        );
        let charges = HashMap::from([(
            "disp-0".to_string(),
            IndianDisposalCharges {
                sale_fee: dec!(40),
                sale_tax: dec!(150),
                purchase_tax: dec!(100),
            },
        )]);
        let summary = run_with_charges(&[s], &charges, 2024);
        let calc = &summary.calculations[0];
        assert_eq!(calc.actual_cost, dec!(100000));
        assert_eq!(calc.proceeds, dec!(150000));
        assert_eq!(calc.sale_price_per_unit, dec!(150000));
        assert_eq!(calc.transfer_expenses, dec!(40));
        assert_eq!(calc.gross_gain, dec!(49960));
        assert_eq!(summary.stcg_total, dec!(49960));
    }

    #[test]
    fn json_field_names_match_the_frontend_types() {
        let v = serde_json::to_value(IndianTaxYearSummary::default()).unwrap();
        for key in [
            "fyStartYear",
            "stcgSlabNet",
            "exemptLtcgSec1038",
            "healthEducationCess",
            "estimatedTotalTax",
            "sec112aLtcgGross",
            "warnings",
            "cleartaxFiles",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(
            serde_json::to_value(IndianTaxSection::Sec10_38).unwrap(),
            "sec10_38"
        );
        assert_eq!(
            serde_json::to_value(IndianTaxSection::SlabStcg).unwrap(),
            "slab_stcg"
        );
        assert_eq!(
            serde_json::to_value(IndianAssetTaxClassification::DebtMf).unwrap(),
            "debt_mf"
        );
    }

    #[test]
    fn cost_inflation_index_and_indexed_cost() {
        assert_eq!(cost_inflation_index(2001), Some(dec!(100)));
        assert_eq!(cost_inflation_index(1995), Some(dec!(100)));
        assert_eq!(cost_inflation_index(2024), Some(dec!(363)));
        assert_eq!(cost_inflation_index(2025), Some(dec!(376)));
        assert_eq!(cost_inflation_index(2099), None);
        // 1,00,000 × 363 / 254
        assert_eq!(
            indexed_cost(dec!(100000), date(2015, 6, 1), date(2024, 5, 1)),
            Some(dec!(142913.39))
        );
    }

    #[test]
    fn pre_budget_section_112_is_taxed_on_the_indexed_gain() {
        use IndianAssetTaxClassification::*;
        // Debt fund bought before 1 Apr 2023, held over 36 months, sold before 23 Jul 2024.
        let summary = run(
            &[sale(
                "debt",
                DebtMf,
                date(2019, 4, 10),
                date(2024, 6, 1),
                dec!(100000),
                dec!(150000),
            )],
            2024,
        );
        let calc = &summary.calculations[0];
        assert_eq!(calc.tax_section, IndianTaxSection::Sec112);
        // 1,00,000 × 363 / 289
        assert_eq!(calc.deemed_cost, dec!(125605.54));
        assert_eq!(calc.gross_gain, dec!(24394.46));
        assert_eq!(summary.ltcg_other_total, dec!(24394.46));
        assert_eq!(summary.estimated_ltcg_tax, dec!(4878.892));
        assert!(summary
            .warnings
            .iter()
            .all(|w| !w.contains("Cost Inflation Index")));
    }

    #[test]
    fn post_budget_section_112_has_no_indexation() {
        use IndianAssetTaxClassification::*;
        let summary = run(
            &[sale(
                "debt",
                DebtMf,
                date(2019, 4, 10),
                date(2024, 8, 1),
                dec!(100000),
                dec!(150000),
            )],
            2024,
        );
        assert_eq!(summary.calculations[0].deemed_cost, dec!(100000));
        assert_eq!(summary.estimated_ltcg_tax, dec!(6250));
    }

    #[test]
    fn listed_sgb_uses_ten_percent_without_indexation_when_lower() {
        use IndianAssetTaxClassification::*;
        // Indexed: 2,00,000 - 1,37,500 = 62,500 at 20% = 12,500.
        // Plain: 1,00,000 at 10% = 10,000, which is lower.
        let summary = run(
            &[sale(
                "sgb",
                GoldSgb,
                date(2016, 8, 1),
                date(2024, 6, 1),
                dec!(100000),
                dec!(200000),
            )],
            2024,
        );
        let calc = &summary.calculations[0];
        assert_eq!(calc.deemed_cost, dec!(100000));
        assert_eq!(calc.applicable_tax_rate, dec!(0.10));
        assert_eq!(summary.estimated_ltcg_tax, dec!(10000));
    }

    #[test]
    fn special_rate_gains_are_listed_after_set_off_and_exemption() {
        use IndianAssetTaxClassification::*;
        let summary = run(
            &[
                sale(
                    "a",
                    EquityListed,
                    date(2023, 1, 1),
                    date(2024, 9, 1),
                    dec!(100000),
                    dec!(300000),
                ),
                sale(
                    "b",
                    EquityListed,
                    date(2024, 6, 1),
                    date(2024, 9, 1),
                    dec!(100000),
                    dec!(150000),
                ),
            ],
            2024,
        );
        assert_eq!(
            summary.special_rate_gains,
            vec![
                IndianSpecialRateGain {
                    section: IndianTaxSection::Sec111a,
                    rate: dec!(0.20),
                    amount: dec!(50000),
                },
                IndianSpecialRateGain {
                    section: IndianTaxSection::Sec112a,
                    rate: dec!(0.125),
                    amount: dec!(75000),
                },
            ]
        );
    }
}
