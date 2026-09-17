# Codebase Overview: Wealthfolio

Wealthfolio is an open-source, local-first personal investment portfolio tracker, spending manager, and net worth simulation platform. It supports desktop (macOS, Windows, Linux, iOS/Android) via Tauri and self-hosted environments via an Axum HTTP server.

---

## 1. System Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│               apps/frontend (React 19 + Vite + Tailwind v4)            │
│                     Dynamic Addons & shadcn UI                         │
└────────────────────────────────────┬───────────────────────────────────┘
                                     │
           ┌─────────────────────────┴─────────────────────────┐
           ▼                                                   ▼
┌──────────────────────┐                           ┌─────────────────────┐
│ apps/tauri (Desktop) │                           │ apps/server (Web)   │
│ Tauri IPC Commands   │                           │ Axum REST API       │
└──────────┬───────────┘                           └──────────┬──────────┘
           │                                                  │
           └─────────────────────────┬────────────────────────┘
                                     ▼
                      ┌─────────────────────────────┐
                      │    crates/core (Domain)     │
                      │  Portfolios, Lots, Goals,   │
                      │  Limits, Activities, FX     │
                      └──────────────┬──────────────┘
                                     │
       ┌──────────────┬──────────────┼──────────────┬──────────────┐
       ▼              ▼              ▼              ▼              ▼
┌──────────────┐┌──────────────┐┌──────────────┐┌──────────────┐┌──────────────┐
│storage-sqlite││ market-data  ││ device-sync  ││   spending   ││      ai      │
│ Diesel ORM   ││ Yahoo, Coin, ││ E2EE Sync,   ││ Transactions,││ LLM Chat,   │
│ & SQLCipher  ││ TwelveData   ││ Enrollment   ││ Rules, Budget││ Tools & MCP  │
└──────────────┘└──────────────┘└──────────────┘└──────────────┘└──────────────┘
```

---

## 2. Directory Layout

### Applications (`apps/`)

| Path | Runtime | Description |
| :--- | :--- | :--- |
| [`/Users/kishore/Workspace/wealthfolio/apps/frontend`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend) | Browser / WebView | React 19 single-page application built with Vite, Tailwind CSS v4, and `@wealthfolio/ui`. |
| [`/Users/kishore/Workspace/wealthfolio/apps/tauri`](file:///Users/kishore/Workspace/wealthfolio/apps/tauri) | Desktop / Mobile | Tauri v2 wrapper exposing native IPC commands, OS keychain secrets, and window management. |
| [`/Users/kishore/Workspace/wealthfolio/apps/server`](file:///Users/kishore/Workspace/wealthfolio/apps/server) | Web / Docker | Axum HTTP server exposing REST APIs for self-hosted instances. |

### Domain Crates (`crates/`)

| Crate | Path | Responsibility |
| :--- | :--- | :--- |
| `core` | [`/Users/kishore/Workspace/wealthfolio/crates/core`](file:///Users/kishore/Workspace/wealthfolio/crates/core) | Core domain models, portfolio valuation, lot-matching (FIFO/LIFO), health metrics, goals, and exports. |
| `storage-sqlite` | [`/Users/kishore/Workspace/wealthfolio/crates/storage-sqlite`](file:///Users/kishore/Workspace/wealthfolio/crates/storage-sqlite) | Diesel ORM repositories, migrations, connection pooling (`r2d2`), and optional SQLCipher encryption. |
| `market-data` | [`/Users/kishore/Workspace/wealthfolio/crates/market-data`](file:///Users/kishore/Workspace/wealthfolio/crates/market-data) | Market data fetchers (Yahoo Finance, CoinGecko, TwelveData, custom providers), symbol lookup, and historical quotes. |
| `connect` | [`/Users/kishore/Workspace/wealthfolio/crates/connect`](file:///Users/kishore/Workspace/wealthfolio/crates/connect) | Wealthfolio Connect cloud integration for automated, read-only brokerage imports. |
| `device-sync` | [`/Users/kishore/Workspace/wealthfolio/crates/device-sync`](file:///Users/kishore/Workspace/wealthfolio/crates/device-sync) | Multi-device sync engine utilizing X25519 key exchange, HKDF, and ChaCha20-Poly1305 E2E encryption. |
| `spending` | [`/Users/kishore/Workspace/wealthfolio/crates/spending`](file:///Users/kishore/Workspace/wealthfolio/crates/spending) | Transaction tracking, automated categorization rules, monthly budgets, and spending insights. |
| `ai` | [`/Users/kishore/Workspace/wealthfolio/crates/ai`](file:///Users/kishore/Workspace/wealthfolio/crates/ai) | Multi-provider LLM integration (OpenAI, Anthropic, Gemini, Ollama, Groq, OpenRouter) with tool calling and chat histories. |
| `agent-tools` | [`/Users/kishore/Workspace/wealthfolio/crates/agent-tools`](file:///Users/kishore/Workspace/wealthfolio/crates/agent-tools) | Domain tools callable by AI agents to query balances, transactions, and portfolio allocations. |
| `wealthfolio-mcp` | [`/Users/kishore/Workspace/wealthfolio/crates/wealthfolio-mcp`](file:///Users/kishore/Workspace/wealthfolio/crates/wealthfolio-mcp) | Model Context Protocol (MCP) server implementation with Personal Access Token (PAT) authentication and audit trails. |
| `http` | [`/Users/kishore/Workspace/wealthfolio/crates/http`](file:///Users/kishore/Workspace/wealthfolio/crates/http) | Shared HTTP client configuration, headers, TLS rustls settings, and proxy utilities. |

### Shared Packages (`packages/`)

| Package | Path | Purpose |
| :--- | :--- | :--- |
| `@wealthfolio/ui` | [`/Users/kishore/Workspace/wealthfolio/packages/ui`](file:///Users/kishore/Workspace/wealthfolio/packages/ui) | Shared React component library built on top of Radix primitives and Tailwind CSS. |
| `addon-sdk` | [`/Users/kishore/Workspace/wealthfolio/packages/addon-sdk`](file:///Users/kishore/Workspace/wealthfolio/packages/addon-sdk) | TypeScript SDK for authoring isolated iframe-based extensions and custom visualizers. |
| `addon-dev-tools` | [`/Users/kishore/Workspace/wealthfolio/packages/addon-dev-tools`](file:///Users/kishore/Workspace/wealthfolio/packages/addon-dev-tools) | Development and mock environment tools for building and testing addons. |

---

## 3. Core Modules & Key Entry Points

### Runtime Adapter Pattern

The frontend maintains dual-runtime parity between desktop (Tauri) and web (Axum) via an adapter abstraction layer:
- Types & Contracts: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/adapters/types.ts`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/adapters/types.ts)
- Parity Test Suite: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/adapters/adapter-command-parity.test.ts`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/adapters/adapter-command-parity.test.ts)
- Vite selects either [`tauri/index.ts`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/adapters/tauri/index.ts) or [`web/index.ts`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/adapters/web/index.ts) at build time via `BUILD_TARGET`.

### Frontend Routes & Pages

Defined in [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/routes.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/routes.tsx):

- **Dashboard / Portfolio**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/dashboard/portfolio-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/dashboard/portfolio-page.tsx)
- **Activities**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/activity/activity-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/activity/activity-page.tsx) and [`activity-import-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/activity/import/activity-import-page.tsx)
- **Holdings**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/holdings/holdings-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/holdings/holdings-page.tsx)
- **Performance & Analytics**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/performance/performance-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/performance/performance-page.tsx)
- **Portfolio Health**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/health/health-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/health/health-page.tsx)
- **Spending & Budget**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/features/spending/pages/spending-insights-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/features/spending/pages/spending-insights-page.tsx)
- **Goals & Retirement**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/features/goals/pages/goals-dashboard-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/features/goals/pages/goals-dashboard-page.tsx)
- **AI Assistant**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/ai-assistant/ai-assistant-page.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/pages/ai-assistant/ai-assistant-page.tsx)
- **Addon Runtime**: [`/Users/kishore/Workspace/wealthfolio/apps/frontend/src/addons/iframe/addon-iframe-route.tsx`](file:///Users/kishore/Workspace/wealthfolio/apps/frontend/src/addons/iframe/addon-iframe-route.tsx)

### Backend Services & Storage

- **Database Schemas & Migrations**: [`/Users/kishore/Workspace/wealthfolio/crates/storage-sqlite/src/schema.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/storage-sqlite/src/schema.rs) and [`migrations/`](file:///Users/kishore/Workspace/wealthfolio/crates/storage-sqlite/migrations)
- **Lot Tracking & Cost Basis**: [`/Users/kishore/Workspace/wealthfolio/crates/storage-sqlite/src/lots.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/storage-sqlite/src/lots.rs)
- **Core Domain Logic**: [`/Users/kishore/Workspace/wealthfolio/crates/core/src/lib.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/core/src/lib.rs)
- **Desktop IPC Command Registration**: [`/Users/kishore/Workspace/wealthfolio/apps/tauri/src/lib.rs`](file:///Users/kishore/Workspace/wealthfolio/apps/tauri/src/lib.rs) and [`commands/mod.rs`](file:///Users/kishore/Workspace/wealthfolio/apps/tauri/src/commands/mod.rs)
- **Web API Handler Routing**: [`/Users/kishore/Workspace/wealthfolio/apps/server/src/api/mod.rs`](file:///Users/kishore/Workspace/wealthfolio/apps/server/src/api/mod.rs)

---

## 4. Key Subsystems

### 1. Portfolio Accounting & Lot Matching
- Calculates position holdings, market values, and cash balances across multiple accounts and currencies.
- Calculates cost basis, realized gains, and unrealized gains across historical buy/sell activities.
- Implements currency conversion (`fx`) against base portfolio currencies.

### 2. Market Data Pipeline
- Manages real-time and historical quote ingestion from multiple providers:
  - Yahoo Finance ([`crates/market-data/src/providers/yahoo.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/market-data/src/providers/yahoo.rs))
  - CoinGecko ([`crates/market-data/src/providers/coingecko.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/market-data/src/providers/coingecko.rs))
  - TwelveData ([`crates/market-data/src/providers/twelvedata.rs`](file:///Users/kishore/Workspace/wealthfolio/crates/market-data/src/providers/twelvedata.rs))
- Supports manual market data import and user-configured custom quote providers.

### 3. Spending & Cash Flow Management
- Categorizes transactions using configurable match rules (merchant matching, regex patterns).
- Computes monthly budgets, recurring transactions, category distributions, and spending trends.

### 4. Device Sync & End-to-End Encryption
- Enables peer-to-peer or relay encrypted synchronization between user devices.
- Uses X25519 static and ephemeral keys combined with ChaCha20-Poly1305 symmetric authenticated encryption.
- Zero plaintext financial data leaves the local SQLite storage without prior encryption.

### 5. AI Assistant & Model Context Protocol (MCP)
- Native conversational financial assistant with multi-provider support (Ollama, OpenAI, Claude, Gemini).
- Built-in function calling using tools defined in [`/Users/kishore/Workspace/wealthfolio/crates/agent-tools`](file:///Users/kishore/Workspace/wealthfolio/crates/agent-tools).
- Standardized MCP server ([`crates/wealthfolio-mcp`](file:///Users/kishore/Workspace/wealthfolio/crates/wealthfolio-mcp)) enabling external AI agents to access portfolio and activity data via authenticated tokens.

---

## 5. Development & Verification Commands

| Command | Action |
| :--- | :--- |
| `pnpm tauri dev` | Launch the Tauri desktop app in development mode |
| `pnpm run dev:web` | Start the frontend and Axum web server in local web mode |
| `pnpm test` | Run TypeScript Vitest test suites across frontend and packages |
| `cargo test` | Run unit and integration tests across all Rust crates |
| `pnpm type-check` | Validate TypeScript types without emitting artifacts |
| `pnpm lint` | Execute ESLint across all packages and apps |
| `pnpm check` | Run the complete project verification pipeline (lint + type check + tests) |
