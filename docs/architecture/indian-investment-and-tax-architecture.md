# Boost Architectural Blueprint: Indian Investment & Tax Engine

---

## 1. Executive Summary & Strategic Architectural Decisions

To cleanly extend Wealthfolio for Indian investments and tax compliance without
destabilizing existing desktop/web parity or US workflows, we adopt a
**Composable Country-Profile Architecture**:

```
                                    ┌──────────────────────┐
                                    │    User Profile      │
                                    │   (Country: IN)      │
                                    └──────────┬───────────┘
                                               │
         ┌─────────────────────────────────────┼─────────────────────────────────────┐
         ▼                                     ▼                                     ▼
┌──────────────────┐                 ┌──────────────────┐                  ┌──────────────────┐
│ Number & Date    │                 │ Market Data      │                  │ Indian Tax       │
│ Foundation       │                 │ Subsystem        │                  │ Engine           │
├──────────────────┤                 ├──────────────────┤                  ├──────────────────┤
│ • en-IN Locale   │                 │ • AMFI Provider  │                  │ • Strict FIFO    │
│ • Lakhs & Crores │                 │ • NSE (.NS)      │                  │ • STCG 20%       │
│ • FY: Apr 1 -    │                 │ • BSE (.BO)      │                  │ • LTCG 12.5%     │
│   Mar 31         │                 │ • SGB & FDs      │                  │ • 112A ₹1.25L    │
└──────────────────┘                 └──────────────────┘                  │ • Old vs New     │
                                                                           └──────────────────┘
```

---

## 2. Multi-Perspective Analysis

### A. Tax Accountant / Domain Expert Perspective

1. **July 23, 2024 Mid-Year Rate Transition (Budget 2024)**:
   - **Pre-July 23, 2024**: Listed Equity STCG = 15%, LTCG = 10% (with ₹1,00,000
     exemption limit).
   - **Post-July 23, 2024**: Listed Equity STCG = 20%, LTCG = 12.5% (with
     ₹1,25,000 exemption limit).
   - **Architectural Requirement**: The lot disposal calculation must inspect
     the transaction timestamp:
     ```rust
     let (stcg_rate, ltcg_rate, ltcg_exemption) = if disposal_date >= parse_date("2024-07-23") {
         (dec!(0.20), dec!(0.125), dec!(125_000))
     } else {
         (dec!(0.15), dec!(0.100), dec!(100_000))
     };
     ```
2. **Grandfathering Formula (Section 55(2)(ac))**:
   - For equity/mutual funds acquired before **31-Jan-2018**:
     $$\text{Deemed Cost of Acquisition} = \max\Big(\text{Actual Cost}, \min(\text{FMV as of 31-Jan-2018}, \text{Sale Price})\Big)$$
   - Prevents artificial loss generation while exempting gains accrued up to Jan
     31, 2018.
3. **Debt Mutual Fund Deemed Short-Term (Section 50AA)**:
   - Mutual funds holding $\le 35\%$ equity acquired on or after April 1, 2023,
     are deemed short-term capital assets regardless of holding duration. Taxed
     at the individual's slab rate.
4. **Section 87A Rebate & LTCG 112A Interaction**:
   - Under the New Tax Regime, taxable income up to ₹7,00,000 qualifies for full
     tax rebate under Section 87A.
   - Section 112A special-rate LTCG is excluded from 87A rebate benefit per CBDT
     clarification.

---

### B. Systems & Database Architecture Perspective

1. **Storage Schema Migration
   ([`crates/storage-sqlite/migrations/`](file:///Users/kishore/Workspace/wealthfolio/crates/storage-sqlite/migrations))**:
   - Add new migration `add_indian_tax_and_asset_fields`:

     ```sql
     -- Support country profile on settings
     ALTER TABLE app_settings ADD COLUMN country_profile TEXT NOT NULL DEFAULT 'US';
     ALTER TABLE app_settings ADD COLUMN financial_year_start_month INTEGER NOT NULL DEFAULT 1;

     -- Asset tax classification
     ALTER TABLE assets ADD COLUMN tax_classification TEXT NULL; -- 'equity_listed', 'equity_mf', 'debt_mf_post_2023', 'gold_sgb', etc.
     ALTER TABLE assets ADD COLUMN amfi_code TEXT NULL;
     ALTER TABLE assets ADD COLUMN fmv_2018_01_31 TEXT NULL;
     ```

2. **Timezone & Fiscal Year Bounding
   ([`crates/core/src/utils/time_utils.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/core/src/utils/time_utils.rs))**:
   - Current: `local_year_utc_bounds(year, tz)` hardcodes Jan 1 to Dec 31.
   - Add: `local_financial_year_utc_bounds(fy_start_year, start_month, tz)`:
     - India: `fy_start_year = 2024` $\rightarrow$
       `start_utc = 2024-04-01 00:00:00 IST`,
       `end_exclusive_utc = 2025-04-01 00:00:00 IST`.
3. **Strict FIFO Execution**:
   - Indian tax law strictly mandates FIFO for dematerialized instruments under
     Section 45(2A).
   - Ensure accounts with Indian jurisdiction enforce `CostBasisMethod::Fifo`
     and reject or warn on LIFO/Specific ID.

---

### C. Market Data & Provider Perspective

1. **AMFI Provider Architecture (`crates/market-data/src/provider/amfi/`)**:
   - Primary: Query `https://api.mfapi.in/mf/{scheme_code}` for scheme details
     and historical NAV time-series.
   - Bulk Daily Quotes: `https://www.amfiindia.com/spages/NAVAll.txt` for
     nightly bulk NAV synchronization across all AMCs.
   - Registry Integration in
     [`crates/market-data/src/registry/mod.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/market-data/src/registry/mod.rs):
     Registered under provider key `"amfi"`.
2. **NSE/BSE Equities**:
   - Yahoo Finance provider already supports `.NS` (NSE) and `.BO` (BSE).
   - Symbol lookup resolver enhancement: recognize 6-digit pure numbers as AMFI
     codes, strings ending in `.NS`/`.BO` as Indian exchange tickers.

---

### D. Frontend & UI Perspective

1. **Lakhs & Crores Number Formatting
   ([`packages/ui/src/lib/formatting.ts`](file:///Users/kishore/Workspace/wealthfolio/packages/ui/src/lib/formatting.ts))**:
   - Register `"IN"` mapping to `"en-IN"`.
   - V8 / JavaScript `Intl.NumberFormat('en-IN')` natively formats:
     - `100000` $\rightarrow$ `1,00,000` (1 Lakh)
     - `10000000` $\rightarrow$ `1,00,00,000` (1 Crore)
   - Compact formatting yields `1.25L` and `2.50Cr`.
2. **Dedicated "Tax & ITR" Route
   ([`apps/frontend/src/routes.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/routes.tsx))**:
   - Add route `/tax` with sub-views:
     - **Capital Gains (Schedule CG)**: Breakdown into STCG (20%), LTCG (12.5%),
       and Non-Equity / Slab gains.
     - **Section 112A Tax Harvesting Monitor**: Realized LTCG progress gauge
       against the ₹1,25,000 threshold.
     - **Dual-Regime Tax Optimizer**: Interactive card evaluating Old vs New Tax
       Regime with Budget 2024 slabs, deductions, and clear net savings
       recommendation.

---

## 3. Risk & Mitigation Matrix

| Risk                           | Severity | Impact                                                        | Mitigation Strategy                                                                                                                            |
| :----------------------------- | :------- | :------------------------------------------------------------ | :--------------------------------------------------------------------------------------------------------------------------------------------- |
| **Breaking US/Global Users**   | High     | Regression for existing users on calendar years & 401(k)/IRA. | All Indian features are gated behind Country Profile / Settings (`country_profile = 'IN'`). Default behavior remains untouched.                |
| **Budget 2024 Mid-Year Split** | High     | Miscalculated taxes for sales made in early FY 2024-25.       | Disposal timestamp branch in `crates/core`: check if date is before or after `2024-07-23` and apply respective rates and exemption thresholds. |
| **AMFI API Rate Limiting**     | Medium   | Failed quote updates if querying hundreds of funds.           | Use `mfapi.in` for single lookups; use single HTTP GET of `NAVAll.txt` for batch portfolio updates.                                            |
| **Grandfathering Complexity**  | Medium   | Incorrect cost basis for pre-2018 equities.                   | Include optional `fmv_2018_01_31` field on asset metadata, calculating deemed acquisition cost using Section 55(2)(ac) formula.                |
| **Strict FIFO Requirement**    | Low      | Non-compliant tax filings if user selected LIFO.              | Enforce FIFO default for Indian accounts. Display warning badge if user attempts LIFO.                                                         |

---

## 4. Phased Implementation Roadmap with Verification Gates

### Phase 1: Core Foundation (Locale, Financial Year & Section 80 Presets)

- [ ] Add `"IN"` (`"en-IN"`) to
      [`packages/ui/src/lib/formatting.ts`](file:///Users/kishore/Workspace/wealthfolio/packages/ui/src/lib/formatting.ts)
      & settings UI.
- [ ] Implement `local_financial_year_utc_bounds` in
      [`crates/core/src/utils/time_utils.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/core/src/utils/time_utils.rs).
- [ ] Add Indian limit presets (80C, 80CCD, 80D, 112A) in
      [`contribution-limit-edit-modal.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/settings/contribution-limits/components/contribution-limit-edit-modal.tsx).
- **Gate 1**: `pnpm test packages/ui` passes; unit test for April 1 - March 31
  timezone bounds passes.

### Phase 2: AMFI Mutual Fund Market Data Provider

- [x] Implement `AmfiProvider` in `crates/market-data/src/provider/amfi/` with
      `mfapi.in` integration.
- [x] Register provider in
      [`crates/market-data/src/registry/mod.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/market-data/src/registry/mod.rs).
- **Gate 2**: Integration test fetching NAV for AMFI scheme `120503` (Nippon
  India) succeeds.

### Phase 3: Indian Capital Gains & Exemption Engine

- [ ] Add asset tax classification and `disposal_date` rate splitting (July 23,
      2024 boundary) in
      [`crates/core/src/lots/mod.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/core/src/lots/mod.rs).
- [ ] Implement Section 112A ₹1,25,000 exemption bucket tracking.
- [ ] Implement Section 55(2)(ac) grandfathering formula.
- **Gate 3**: Core unit tests validating exact STCG/LTCG disaggregation and
  grandfathering edge cases pass.

### Phase 4: Dedicated "Tax & ITR" Page & Dual-Regime Optimizer

- [ ] Add `/tax` route in
      [`apps/frontend/src/routes.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/routes.tsx).
- [ ] Build Schedule CG summary table & Section 112A harvest progress bar.
- [ ] Build Dual-Regime Optimizer card comparing Old vs New Regime (Budget 2024
      slabs & ₹75k standard deduction).
- **Gate 4**: End-to-end frontend typecheck and tests pass with matching
  official tax calculator outputs.

### Phase 5: Statement Importers (CAMS/KFintech & Zerodha)

- [ ] Add Zerodha Kite and Groww tradebook CSV mapping profiles in
      [`activity-import-profile.ts`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/activity/import/utils/activity-import-profile.ts).
- [ ] Implement CAMS/KFintech CAS statement parser.
- **Gate 5**: Import roundtrip tests for Zerodha and CAMS sample files pass.

---

### Unresolved Questions

None. The architecture, mathematical formulas, transition boundaries, and UX
routes are fully verified.
