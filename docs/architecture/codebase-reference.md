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
