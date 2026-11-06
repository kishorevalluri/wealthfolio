# Wealthfolio Repository Architecture & Data Flow Reference

Quick reference guide for agent context to avoid full codebase re-scans for common operations.

## Architecture Layers & Data Flow

```
Frontend (React 19 + Vite + TanStack Query)
  ├── Pages: apps/frontend/src/pages/
  │     ├── asset/ (Securities, details, quotes)
  │     ├── settings/ (Securities, Market Data, Accounts)
  │     └── ...
  ├── Hooks / Data Fetching: apps/frontend/src/hooks/, apps/frontend/src/pages/*/hooks/
  ├── Adapter Layer: apps/frontend/src/adapters/
  │     ├── tauri/ (Tauri IPC invoke)
  │     ├── web/ (Axum HTTP fetch)
  │     └── shared/ (Common API signatures)
  └── Backend Call Wrappers: apps/frontend/src/commands/

Desktop Mode: Tauri IPC (apps/tauri/src/commands/)
Web Mode: Axum HTTP (apps/server/src/api/)
  └── crates/core/ (Domain logic, services, models)
        ├── quotes/ (Market data coordinator, registry, provider settings)
        ├── assets/ (Asset management)
        ├── transactions/ (Activities, portfolio sync)
        └── ...
        └── crates/market-data/ (Providers: AMFI, Yahoo, CoinGecko, Fixture)
        └── crates/storage-sqlite/ (Diesel ORM, schema, migrations)
```

## Market Data & Quotes Subsystem

### Builtin Providers
1. **AMFI** (`MarketDataProvider::Amfi` / `amfi`)
   - Type: Mutual Funds in India (NAV data from AMFI daily NAV text feed `portal.amfiindia.com/spages/NAVAll.txt` and mfapi.in).
   - ISIN/Scheme Code matching, daily NAV quotes + history.
  - Provider priority and settings in `crates/core/src/quotes/provider_settings.rs` and `crates/market-data/src/provider/amfi/`.
   - Empty name searches fall back to the mfapi `/mf` scheme catalogue, cached in memory per provider for 24 hours with serialized refresh. Numeric codes and Indian ISINs bypass this fallback.
   - `crates/market-data/src/provider/amfi/nav_catalogue.rs` parses the public AMFI `NAVAll.txt` feed by header names, supporting legacy six-column and current eight-column layouts (separate Plan/Option). Exact `INF` ISIN searches use both growth/payout and reinvestment columns before name search, with an independent serialized 24-hour in-memory cache. Invalid feeds and refresh/network failures return errors; an absent ISIN returns no candidates. This daily NAV feed is not a complete historical/security master.
   - Provider search results carry optional verified `isin` through `MarketDataClient` conversion and quote-service existing-asset deduplication, preserving provider identity/type alongside the local asset ID. Import derives an ISIN from the source symbol when no separate identifier is supplied, accepts unique exact ISIN evidence without requiring name variants, rejects explicit mismatched ISINs, and stops automatic name fallback on conflicting provider identities. Duplicate catalogue rows for one scheme code deduplicate; different scheme codes for one ISIN stay manual-review candidates.
   - ETF search/profile classification uses explicit AMFI/mfapi categories (`Exchange Traded Funds (ETFs)`, legacy `Other Scheme - Gold ETF` / `Other Scheme - Other ETFs`), not an `INF` prefix or ETF-like name. Index funds and funds of funds retain mutual-fund classification; exact ETF ISIN matches do not require Direct/Regular or Growth/IDCW markers.
   - `crates/market-data/src/provider/amfi/matching.rs` compares spacing/punctuation-insensitive scheme names and edit distance, tolerates redundant Plan/Option suffixes, and rejects conflicting Direct/Regular, Growth/IDCW, and payout/reinvestment variants. Meaningful base-name words and numeric series/index identifiers are preserved; differing number runs reject the match. Fallback candidates require similarity >= 0.90; search returns the top 20 for manual selection.
   - Import auto-selection in `crates/core/src/assets/assets_service.rs` accepts AMFI name matches only with complete matching variants, similarity >= 0.90, and a >= 0.05 lead over competing scheme codes. Unique verified provider ISINs bypass the name guard; exact provider codes and existing-asset ISIN resolution keep their existing precedence. Ambiguous names stay unresolved for manual review.
2. **Yahoo Finance** (`MarketDataProvider::YahooFinance` / `yahoo`)
   - Equities, ETFs, Indices, FX across global exchanges (.NS, .BO for India).
3. **CoinGecko** (`MarketDataProvider::CoinGecko` / `coingecko`)
   - Crypto assets.
4. **Fixture** (`MarketDataProvider::Fixture` / `fixture`)
   - Testing/mocking provider.

### Query Invalidation Rules (TanStack Query)
TanStack Query matches invalidation by key prefix array.
- Asset list: `[QueryKeys.ASSETS]`
- Asset individual details: `[QueryKeys.ASSET_DATA, assetId]`
- Holdings: `[QueryKeys.HOLDINGS]`
- Quote history: `[QueryKeys.QUOTE_HISTORY]`
- Latest quotes:
  - Registered hook query key in `useLatestQuotes`: `[QueryKeys.ASSETS, QueryKeys.LATEST_QUOTES, sortedSymbolsKey]`
  - IMPORTANT: Invalidation MUST target `[QueryKeys.ASSETS, QueryKeys.LATEST_QUOTES]` (and optionally `[QueryKeys.LATEST_QUOTES]`).
  - Invalidation locations:
    - `apps/frontend/src/hooks/use-sync-market-data.ts` (Sync / Update Quotes)
    - `apps/frontend/src/pages/asset/hooks/use-asset-management.ts` (Create, Delete, Update Asset)

### Provider Priority in Multi-Provider Search
Search merging in `crates/core/src/quotes/service.rs`:
- Providers are sorted by user-configured priority (`enabled_providers()` order) first.
- Search results preserve provider ordering before score sorting, respecting user preferences for Indian securities (e.g., AMFI or Yahoo/BSE/NSE).

### Key DB Tables & Migrations (`crates/storage-sqlite/`)
- `assets`: Securities / instruments (`id`, `symbol`, `name`, `quote_provider`, `quote_symbol`, `asset_sub_class`, etc.)
- `quotes`: Historical prices (`asset_id`, `timestamp`, `price`, `open`, `high`, `low`, `volume`)
- `provider_settings`: Market data provider configuration (`provider`, `priority`, `enabled`)

## Indian Capital Gains Tax / ITR Subsystem

### Overview
Phase 4 feature: compute Indian capital gains summaries for ITR filing. Visible only when `settings.formattingRegion === "IN"` or `settings.baseCurrency === "INR"`.

### Data Flow
```
Frontend /tax route → TaxPage
  → getIndianTaxSummary(filter?, financialYear?)       [apps/frontend/src/adapters/shared/portfolio.ts]
  → invoke("get_indian_tax_summary", { filter, financialYear })
  → [Desktop] Tauri IPC → commands::portfolio::get_indian_tax_summary
  → [Web]     POST /tax/indian/summary/query           [apps/server/src/api/tax.rs]
  → compute_indian_tax_summary()                       [crates/core/src/lots/tax_service.rs]
  → lots_repo.get_lot_disposals_for_accounts_in_date_range()
  → lots_repo.get_all_lots()
  → asset_service.get_assets()
  → classify_asset_for_indian_tax() per asset
  → calculate_indian_capital_gains()                   [crates/core/src/lots/mod.rs]
  → IndianTaxYearSummary

Frontend /tax route Export Button →
  → exportIndianScheduleCg(filter?, financialYear?)    [apps/frontend/src/adapters/*/exports.ts]
  → [Desktop] Tauri IPC → commands::portfolio::export_indian_schedule_cg
  → [Web]     POST /tax/indian/schedule-cg/export/query [apps/server/src/api/tax.rs]
  → to_schedule_cg_rows()                              [crates/core/src/lots/tax_export.rs]
  → format_records(&rows, ExportFileFormat::Csv)       [crates/core/src/exports.rs]
  → CSV File Download (native dialog or browser download)
```

### Key Files
| File | Purpose |
|---|---|
| `crates/core/src/lots/tax_service.rs` | `classify_asset_for_indian_tax`, `extract_fmv_2018`, `compute_indian_tax_summary` |
| `crates/core/src/lots/mod.rs` | `IndianAssetTaxClassification`, `IndianTaxYearSummary`, `calculate_indian_capital_gains` |
| `crates/core/src/lots/tax_export.rs` | `ScheduleCgRow`, `to_schedule_cg_rows` (ITR-2/ITR-3 compliant CSV generator) |
| `apps/tauri/src/commands/portfolio.rs` | `get_indian_tax_summary`, `export_indian_schedule_cg` Tauri IPC commands |
| `apps/server/src/api/tax.rs` | `GET/POST /tax/indian/summary`, `GET/POST /tax/indian/schedule-cg/export` |
| `apps/frontend/src/pages/tax/tax-page.tsx` | UI: FY selector, export button, Section 112A monitor, disposal table with security names/ISINs |
| `apps/frontend/src/pages/tax/components/dual-regime-optimizer.tsx` | Dual-Regime Optimizer: Old vs New Tax Regime with Budget 2024 slabs & ₹75k standard deduction |
| `apps/frontend/src/adapters/shared/portfolio.ts` | `getIndianTaxSummary` command wrapper |
| `apps/frontend/src/adapters/{tauri,web}/exports.ts` | `exportIndianScheduleCg` command implementation |
| `apps/frontend/src/lib/types.ts` | `IndianTaxYearSummary`, `IndianCapitalGainCalculation` TS types with export fields |
| `apps/frontend/src/pages/layouts/navigation/app-navigation.tsx` | Conditional Tax & ITR nav item (India only) |

### Query Key
`QueryKeys.INDIAN_TAX_SUMMARY` / `QueryKeys.indianTaxSummary(filter, financialYear?)`

### Asset Tax Classification
`classify_asset_for_indian_tax` reads metadata key `taxClassification` first, then falls back to `instrument_type` + MIC heuristics. FMV (for grandfathering Sec 55(2)(ac)) is read from metadata keys: `fmv20180131`, `fmv_2018_01_31`, or `fmv2018`.

### Financial Year
FY start year passed as `financial_year: i32` (e.g. 2024 → FY 2024-25). Defaults to current FY based on April cutoff if not supplied.

## Indian Broker & Statement Import Subsystem

### Pre-seeded System Templates (`crates/storage-sqlite/migrations/`)
- **`system_zerodha`** (Zerodha Kite Tradebook):
  - Field mappings: `trade_date`, `trade_type`, `symbol`, `isin`, `quantity`, `price`, `trade_id`, `segment`.
  - Activities: `buy` -> BUY, `sell` -> SELL.
- **`system_groww`** (Groww Tradebook):
  - Field mappings: `Execution Date`, `Type`, `Symbol`, `ISIN`, `Quantity`, `Price`, `Order Value`, `Order Status`.
  - Activities: `BUY` -> BUY, `SELL` -> SELL.
- **`system_cams_cas`** (CAMS / KFintech Consolidated Account Statement):
  - Field mappings: `Date`, `Transaction Description`, `Scheme Name`, `ISIN`, `Units`, `NAV`, `Amount`, `Folio No`.
  - Activities: `PURCHASE`/`SIP`/`ADDITIONAL PURCHASE`/`SWITCH IN`/`REINVESTMENT` -> BUY, `REDEMPTION`/`SWITCH OUT`/`SWP` -> SELL, `DIVIDEND PAYOUT` -> DIVIDEND, `STAMP DUTY`/`STT`/`TDS` -> TAX, `BROKERAGE`/`CHARGES` -> FEE.
- **`system_cdsl_cas`** (CDSL e-CAS - Central Depository Services Limited):
  - Field mappings: `Date`, `Transaction Description`, `Security Description`, `ISIN`, `Quantity`, `Rate`, `Amount`, `BO ID`.
  - Activities: `PURCHASE`/`BUY`/`OFF-MARKET INWARD`/`IPO ALLOTMENT`/`BONUS`/`RIGHTS`/`INTER-DEPOSITORY TRANSFER INWARD`/`TRANSMISSION INWARD` -> BUY, `SALE`/`SELL`/`OFF-MARKET OUTWARD`/`REDEMPTION`/`INTER-DEPOSITORY TRANSFER OUTWARD`/`TRANSMISSION OUTWARD` -> SELL, `STOCK SPLIT`/`SUBDIVISION` -> SPLIT, `CHARGES`/`DEPOSITORY CHARGES` -> FEE.
- **`system_nsdl_cas`** (NSDL e-CAS - National Securities Depository Limited):
  - Field mappings: `Transaction Date`, `Particulars`, `Security Name`, `ISIN`, `Quantity`, `Rate`, `Amount`, `Client ID`.
  - Activities: `BUY`/`PURCHASE`/`OFF-MARKET INWARD`/`IPO ALLOTMENT`/`BONUS`/`RIGHTS`/`INTER-DEPOSITORY TRANSFER INWARD`/`DEMAT INWARD` -> BUY, `SELL`/`SALE`/`OFF-MARKET OUTWARD`/`REDEMPTION`/`INTER-DEPOSITORY TRANSFER OUTWARD`/`DEMAT OUTWARD` -> SELL, `SUB-DIVISION`/`STOCK SPLIT` -> SPLIT, `CHARGES`/`DEP CHARGES` -> FEE.

### CSV Auto-detection
Configured in `apps/frontend/src/pages/activity/import/hooks/use-import-mapping.ts` (`COLUMN_ALIASES`). Supports Indian tradebook/CAS column titles like `Execution Date`, `Transaction Description`, `Particulars`, `Scheme Name`, `Security Description`, `Security Name`, `Order Value`, `Folio No`, `BO ID`, `Client ID`, `DP ID`, `Rate`, `NAV`, etc.

### Native PDF CAS Import & Statement Parsers (CAMS, KFintech, CDSL, NSDL, Zerodha)
- **Modular Parsers Architecture**: Located in [`crates/core/src/activities/parsers/`](crates/core/src/activities/parsers/):
  - `cdsl_cas.rs`: CDSL e-CAS parser with bilingual Devanagari / Hindi filtering, portfolio total summary exclusion, corporate description cleaning, and holdings/transaction extraction.
  - Empty transaction security descriptions now reuse the pre-scanned statement `mf_scheme_map` for the same extracted ISIN, including scheme sections occurring later in the text and both inline/wrapped transaction layouts. Non-empty inline names remain unchanged; unknown ISINs remain unnamed. This does not repair incorrect/missing transaction ISIN context or infer missing scheme variants.
  - Real encrypted-statement revalidation exercised the actual Rust `parse_pdf` and live AMFI exact-ISIN search: all extracted transaction descriptions were non-empty and every distinct extracted `INF` ISIN returned one catalogue scheme, including an explicitly classified ETF. The temporary aggregate-only harness was removed; no password or decrypted records were saved in fixtures. This verifies parser/provider behavior, not the full import wizard or asset-resolution service against private data. Source completeness and extracted ISIN context still require independent page-by-page verification. Some extracted names omit plan/option markers; fuzzy matching cannot infer missing variants safely. An `INF` prefix alone does not distinguish mutual funds from ETFs, and row counts do not prove source completeness.
  - Follow-up validation against a fresh isolated web database exercised the real multipart PDF endpoint successfully. Independent in-memory PyMuPDF extraction found text on all 22 pages and 49 distinct source ISINs versus 46 distinct parsed ISINs; three source `INF` identifiers are absent from parsed rows and require investigation before claiming completeness. All 46 parsed holding quantities, rates, and amounts occurred near the same ISIN in independent source text, but proximity is not a semantic/column-level reconciliation. Browser wizard validation was blocked before PDF upload by missing Chrome; attempted Playwright Chromium installation timed out and left an unusable browser. The original `.env.web` was restored; no private data was committed/imported and no private screenshots/traces were created.
  - `cams_kfin_cas.rs`: CAMS and KFintech mutual fund CAS parser.
  - `nsdl_cas.rs`: NSDL e-CAS depository parser.
  - `zerodha_kite.rs`: Zerodha Kite tradebook and statement parser.
  - `generic_pdf.rs`: Generic fallback extractor for unknown statements.
- **Local Decryption & Text Parsing**: Handled in [`crates/core/src/activities/pdf_parser.rs`](crates/core/src/activities/pdf_parser.rs) using `lopdf`. Zero cloud transmission, privacy-first.
- **Encrypted PDF Handling**:
  - Encrypted PDFs prompt the user via `PdfPasswordDialog` ([`pdf-password-dialog.tsx`](apps/frontend/src/pages/activity/import/components/pdf-password-dialog.tsx)) with hints (PAN / DOB / BOID / Client ID).
  - Passes password to `parse_pdf` Tauri command / Axum endpoint (`/activities/import/parse-pdf`).
- **Statement Type Detection**: Detects CAMS CAS, KFintech CAS, CDSL e-CAS, and NSDL e-CAS, parsing tabular data into standard `ParsedCsvResult` and auto-selecting the corresponding system template (`system_cams_cas`, `system_cdsl_cas`, `system_nsdl_cas`).
- **FileDropzone**: Accepts `.csv,.pdf` in [`upload-step.tsx`](apps/frontend/src/pages/activity/import/steps/upload-step.tsx).
- **Holdings Mapping Auto-Detection**: Auto-detects `Date`, `ISIN`/`Security Description` as symbol, `Quantity`/`Units`, and `Rate`/`NAV`/`Price` as average cost in [`holdings-mapping-step.tsx`](apps/frontend/src/pages/activity/import/steps/holdings-mapping-step.tsx).
