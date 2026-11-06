//! AMFI (Association of Mutual Funds in India) market data provider.
//!
//! Fetches NAV quotes and scheme profiles for Indian mutual funds using:
//! - Scheme details & historical NAVs: `https://api.mfapi.in/mf/{scheme_code}`
//! - Latest quotes: `https://api.mfapi.in/mf/{scheme_code}/latest`
//! - Search: `https://api.mfapi.in/mf/search?q={query}`
//!
//! No API key is required.

use std::borrow::Cow;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;

pub mod matching;
mod nav_catalogue;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::errors::MarketDataError;
use crate::models::{
    AssetProfile, Coverage, InstrumentKind, ProviderId, ProviderInstrument, Quote, QuoteContext,
    SearchResult,
};
use crate::provider::{MarketDataProvider, ProviderCapabilities, RateLimit};
use crate::registry::{RateLimitConfig, RateLimiter};

pub const PROVIDER_ID: &str = "AMFI";
const BASE_URL: &str = "https://api.mfapi.in";
const NAV_CATALOGUE_URL: &str = "https://portal.amfiindia.com/spages/NAVAll.txt";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const CATALOGUE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MfMeta {
    pub fund_house: Option<String>,
    pub scheme_type: Option<String>,
    pub scheme_category: Option<String>,
    pub scheme_code: Option<i64>,
    pub scheme_name: Option<String>,
    pub isin_growth: Option<String>,
    pub isin_div_reinvestment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MfDataPoint {
    pub date: String,
    pub nav: String,
}

#[derive(Debug, Deserialize)]
pub struct MfResponse {
    pub meta: Option<MfMeta>,
    #[serde(default)]
    pub data: Vec<MfDataPoint>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MfSearchResultItem {
    #[serde(rename = "schemeCode")]
    pub scheme_code: i64,
    #[serde(rename = "schemeName")]
    pub scheme_name: String,
}

// ---------------------------------------------------------------------------
// Provider struct
// ---------------------------------------------------------------------------

pub struct AmfiProvider {
    client: Client,
    request_limiter: RateLimiter,
    catalogue: Mutex<Option<(Instant, Vec<MfSearchResultItem>)>>,
    nav_catalogue: Mutex<Option<(Instant, Vec<nav_catalogue::NavScheme>)>>,
}

impl Default for AmfiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AmfiProvider {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(UA));

        let client = wealthfolio_http::client_builder()
            .timeout(REQUEST_TIMEOUT)
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| wealthfolio_http::client());

        let request_limiter = RateLimiter::new();
        let provider_id: ProviderId = Cow::Borrowed(PROVIDER_ID);
        request_limiter.configure(
            &provider_id,
            RateLimitConfig {
                requests_per_minute: 60,
                burst_capacity: 5.0,
            },
        );

        Self {
            client,
            request_limiter,
            catalogue: Mutex::new(None),
            nav_catalogue: Mutex::new(None),
        }
    }

    async fn send_get(&self, url: &str) -> Result<reqwest::Response, MarketDataError> {
        let provider_id: ProviderId = Cow::Borrowed(PROVIDER_ID);
        self.request_limiter.acquire(&provider_id).await;
        self.client
            .get(url)
            .send()
            .await
            .map_err(|e| MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("HTTP request failed: {}", e),
            })
    }

    fn extract_scheme_code(instrument: &ProviderInstrument) -> Result<String, MarketDataError> {
        match instrument {
            ProviderInstrument::EquitySymbol { symbol } => {
                let s = symbol.trim();
                // AMFI scheme codes are numeric (e.g. "120503")
                if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
                    Ok(s.to_string())
                } else {
                    Err(MarketDataError::UnsupportedAssetType(format!(
                        "AMFI provider expects numeric scheme code, got '{}'",
                        symbol
                    )))
                }
            }
            _ => Err(MarketDataError::UnsupportedAssetType(format!(
                "{:?}",
                instrument
            ))),
        }
    }

    fn parse_nav_date(d: &str) -> Option<DateTime<Utc>> {
        // Date format is "DD-MM-YYYY" (e.g., "16-09-2026")
        let naive = NaiveDate::parse_from_str(d.trim(), "%d-%m-%Y").ok()?;
        // Close of day in IST (15:30 IST is 10:00 UTC)
        let dt = naive.and_hms_opt(10, 0, 0)?;
        Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
    }

    fn profile_from_meta(meta: MfMeta) -> AssetProfile {
        let quote_type = if meta
            .scheme_category
            .as_deref()
            .is_some_and(nav_catalogue::is_etf_category)
        {
            "ETF"
        } else {
            "MUTUALFUND"
        };
        let mut profile = AssetProfile {
            source: Some(PROVIDER_ID.to_string()),
            name: meta.scheme_name,
            quote_type: Some(quote_type.to_string()),
            currency: Some("INR".to_string()),
            exchange: Some("AMFI".to_string()),
            sector: meta.scheme_category,
            description: meta.scheme_type,
            country: Some("IN".to_string()),
            ..Default::default()
        };
        if let Some(isin) = meta.isin_growth.or(meta.isin_div_reinvestment) {
            if !isin.is_empty() {
                profile.isin = Some(isin);
            }
        }
        profile
    }
}

// ---------------------------------------------------------------------------
// MarketDataProvider implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl MarketDataProvider for AmfiProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn priority(&self) -> u8 {
        5
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            instrument_kinds: &[InstrumentKind::Equity],
            coverage: Coverage::indian_market(),
            supports_latest: true,
            supports_historical: true,
            supports_search: true,
            supports_profile: true,
            supports_dividends: false,
        }
    }

    fn rate_limit(&self) -> RateLimit {
        RateLimit {
            requests_per_minute: 60,
            max_concurrency: 5,
            min_delay: Duration::from_millis(200),
        }
    }

    async fn get_latest_quote(
        &self,
        context: &QuoteContext,
        instrument: ProviderInstrument,
    ) -> Result<Quote, MarketDataError> {
        let scheme_code = Self::extract_scheme_code(&instrument)?;
        let url = format!("{}/mf/{}/latest", BASE_URL, scheme_code);
        debug!("AMFI get_latest_quote: {}", url);

        let resp = self.send_get(&url).await?;
        if !resp.status().is_success() {
            return Err(MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("HTTP {}", resp.status()),
            });
        }

        let body: MfResponse = resp
            .json()
            .await
            .map_err(|e| MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("JSON parse error: {}", e),
            })?;

        // mfapi returns status: "SUCCESS" with empty data and code 0 if not found
        if body.data.is_empty()
            || body
                .meta
                .as_ref()
                .and_then(|m| m.scheme_code)
                .unwrap_or(0)
                == 0
        {
            return Err(MarketDataError::SymbolNotFound(scheme_code));
        }

        let point = &body.data[0];
        let timestamp = Self::parse_nav_date(&point.date).ok_or_else(|| {
            MarketDataError::ValidationFailed {
                message: format!("Invalid date format in AMFI response: {}", point.date),
            }
        })?;

        let close: Decimal =
            point
                .nav
                .trim()
                .parse()
                .map_err(|_| MarketDataError::ValidationFailed {
                    message: format!("Invalid NAV format in AMFI response: {}", point.nav),
                })?;

        let currency = context
            .currency_hint
            .as_ref()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "INR".to_string());

        Ok(Quote {
            timestamp,
            open: None,
            high: None,
            low: None,
            close,
            volume: None,
            currency,
            source: PROVIDER_ID.to_string(),
        })
    }

    async fn get_historical_quotes(
        &self,
        context: &QuoteContext,
        instrument: ProviderInstrument,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Quote>, MarketDataError> {
        let scheme_code = Self::extract_scheme_code(&instrument)?;
        let url = format!("{}/mf/{}", BASE_URL, scheme_code);
        debug!("AMFI get_historical_quotes: {}", url);

        let resp = self.send_get(&url).await?;
        if !resp.status().is_success() {
            return Err(MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("HTTP {}", resp.status()),
            });
        }

        let body: MfResponse = resp
            .json()
            .await
            .map_err(|e| MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("JSON parse error: {}", e),
            })?;

        if body.data.is_empty()
            || body
                .meta
                .as_ref()
                .and_then(|m| m.scheme_code)
                .unwrap_or(0)
                == 0
        {
            return Err(MarketDataError::SymbolNotFound(scheme_code));
        }

        let currency = context
            .currency_hint
            .as_ref()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "INR".to_string());

        let mut quotes = Vec::new();

        for point in &body.data {
            let Some(ts) = Self::parse_nav_date(&point.date) else {
                continue;
            };

            if ts >= start && ts <= end {
                if let Ok(close) = point.nav.trim().parse::<Decimal>() {
                    quotes.push(Quote {
                        timestamp: ts,
                        open: None,
                        high: None,
                        low: None,
                        close,
                        volume: None,
                        currency: currency.clone(),
                        source: PROVIDER_ID.to_string(),
                    });
                }
            }
        }

        // Sort ascending by timestamp
        quotes.sort_by_key(|q| q.timestamp);

        if quotes.is_empty() {
            return Err(MarketDataError::NoDataForRange);
        }

        Ok(quotes)
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, MarketDataError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(vec![]);
        }

        let normalized = trimmed.to_ascii_uppercase();
        if normalized.len() == 12
            && normalized.starts_with("IN")
            && normalized.chars().all(|c| c.is_ascii_alphanumeric())
        {
            if !normalized.starts_with("INF") {
                return Ok(vec![]);
            }
            let mut catalogue = self.nav_catalogue.lock().await;
            if catalogue
                .as_ref()
                .is_none_or(|(loaded, _)| loaded.elapsed() >= CATALOGUE_TTL)
            {
                let response = self.send_get(NAV_CATALOGUE_URL).await?;
                if !response.status().is_success() {
                    return Err(MarketDataError::ProviderError {
                        provider: PROVIDER_ID.to_string(),
                        message: format!("NAV catalogue HTTP {}", response.status()),
                    });
                }
                let text = response
                    .text()
                    .await
                    .map_err(|e| MarketDataError::ProviderError {
                        provider: PROVIDER_ID.to_string(),
                        message: format!("NAV catalogue response error: {}", e),
                    })?;
                let schemes = nav_catalogue::parse(&text)?;
                *catalogue = Some((Instant::now(), schemes));
            }
            return Ok(catalogue
                .as_ref()
                .map(|(_, schemes)| nav_catalogue::lookup(schemes, &normalized))
                .unwrap_or_default());
        }

        // AMFI (api.mfapi.in) is strictly for mutual funds.
        // Skip queries that are clearly exchange stock tickers or demat / account IDs.
        if trimmed.ends_with(".NS")
            || trimmed.ends_with(".BO")
            || trimmed.contains('/')
            || (trimmed.len() > 10 && trimmed.chars().all(|c| c.is_ascii_digit()))
        {
            return Ok(vec![]);
        }

        let url = format!("{}/mf/search?q={}", BASE_URL, urlencoding::encode(trimmed));
        debug!("AMFI search: {}", url);

        let resp = self.send_get(&url).await?;
        if !resp.status().is_success() {
            return Err(MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("HTTP {}", resp.status()),
            });
        }

        let mut items: Vec<MfSearchResultItem> =
            resp.json()
                .await
                .map_err(|e| MarketDataError::ProviderError {
                    provider: PROVIDER_ID.to_string(),
                    message: format!("JSON parse error: {}", e),
                })?;

        if items.is_empty()
            && !trimmed.chars().all(|c| c.is_ascii_digit())
            && trimmed.len() >= 8
            && !(trimmed.len() == 12
                && trimmed.starts_with("IN")
                && trimmed.chars().all(|c| c.is_ascii_alphanumeric()))
        {
            let mut catalogue = self.catalogue.lock().await;
            if catalogue
                .as_ref()
                .is_none_or(|(loaded, _)| loaded.elapsed() >= CATALOGUE_TTL)
            {
                let response = self.send_get(&format!("{}/mf", BASE_URL)).await?;
                if !response.status().is_success() {
                    return Err(MarketDataError::ProviderError {
                        provider: PROVIDER_ID.to_string(),
                        message: format!("Catalogue HTTP {}", response.status()),
                    });
                }
                let schemes = response
                    .json::<Vec<MfSearchResultItem>>()
                    .await
                    .map_err(|e| MarketDataError::ProviderError {
                        provider: PROVIDER_ID.to_string(),
                        message: format!("Catalogue JSON parse error: {}", e),
                    })?;
                *catalogue = Some((Instant::now(), schemes));
            }
            if let Some((_, schemes)) = catalogue.as_ref() {
                items = schemes
                    .iter()
                    .filter(|item| matching::name_similarity(trimmed, &item.scheme_name) >= 0.9)
                    .cloned()
                    .collect();
            }
        }

        let results = items
            .into_iter()
            .map(|item| {
                let code_str = item.scheme_code.to_string();
                let score = if code_str.eq_ignore_ascii_case(trimmed) {
                    25000.0
                } else {
                    15000.0 + 5000.0 * matching::name_similarity(trimmed, &item.scheme_name)
                };

                SearchResult::new(code_str, item.scheme_name, "AMFI", "MUTUALFUND")
                    .with_currency("INR")
                    .with_data_source(PROVIDER_ID)
                    .with_score(score)
            })
            .collect();

        let mut results: Vec<SearchResult> = results;
        results.sort_by(|a, b| {
            b.score
                .unwrap_or_default()
                .total_cmp(&a.score.unwrap_or_default())
        });
        results.truncate(20);
        Ok(results)
    }

    async fn get_profile(&self, symbol: &str) -> Result<AssetProfile, MarketDataError> {
        let trimmed = symbol.trim();
        let url = format!("{}/mf/{}/latest", BASE_URL, trimmed);
        debug!("AMFI get_profile: {}", url);

        let resp = self.send_get(&url).await?;
        if !resp.status().is_success() {
            return Err(MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("HTTP {}", resp.status()),
            });
        }

        let body: MfResponse = resp
            .json()
            .await
            .map_err(|e| MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("JSON parse error: {}", e),
            })?;

        let meta = body
            .meta
            .filter(|m| m.scheme_code.unwrap_or(0) != 0)
            .ok_or_else(|| MarketDataError::SymbolNotFound(trimmed.to_string()))?;

        Ok(Self::profile_from_meta(meta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_classifies_etf_only_from_explicit_category() {
        for (category, expected) in [
            ("Exchange Traded Funds (ETFs) - Debt ETF", "ETF"),
            ("Other Scheme - Gold ETF", "ETF"),
            ("Other Scheme - Other ETFs", "ETF"),
            ("Other Scheme - FoF Domestic", "MUTUALFUND"),
            ("Equity Scheme - Index Fund", "MUTUALFUND"),
        ] {
            let meta: MfMeta = serde_json::from_value(serde_json::json!({
                "scheme_code": 100001,
                "scheme_name": "Example ETF Fund of Funds",
                "scheme_category": category,
                "isin_growth": "INF000000001"
            }))
            .unwrap();
            let profile = AmfiProvider::profile_from_meta(meta);
            assert_eq!(profile.quote_type.as_deref(), Some(expected));
        }
    }

    fn offline_provider() -> AmfiProvider {
        let mut provider = AmfiProvider::new();
        provider.client = Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        provider
    }

    #[tokio::test]
    async fn isin_search_uses_cached_feed_without_name_search() {
        let provider = offline_provider();
        let schemes = nav_catalogue::parse("Scheme Code;ISIN Div Payout/ ISIN Growth;ISIN Div Reinvestment;Scheme Name;Plan;Option;Net Asset Value;Date\nOpen Ended Schemes(Exchange Traded Funds (ETFs) - Gold ETF)\n100001;INF000000001;INF000000002;Example Gold;Regular Plan;Growth Option;10;01-Oct-2026\n").unwrap();
        *provider.nav_catalogue.lock().await = Some((Instant::now(), schemes));
        let results = provider.search(" inf000000002 ").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol, "100001");
        assert_eq!(results[0].isin.as_deref(), Some("INF000000002"));
        assert_eq!(results[0].asset_type, "ETF");
        assert!(provider.search("INF000000003").await.unwrap().is_empty());
        assert!(provider.search("INE000000001").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn isin_search_reports_feed_failure_instead_of_exact_match() {
        let provider = offline_provider();
        assert!(provider.search("INF000000001").await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires the live public AMFI NAV catalogue"]
    async fn live_nav_catalogue_resolves_public_scheme_isin() {
        let provider = AmfiProvider::new();
        let results = provider.search("INF846K01WO1").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol, "135762");
        assert_eq!(results[0].isin.as_deref(), Some("INF846K01WO1"));
        assert_eq!(results[0].asset_type, "MUTUALFUND");
    }

    #[test]
    fn test_extract_scheme_code() {
        let inst = ProviderInstrument::EquitySymbol {
            symbol: "120503".into(),
        };
        assert_eq!(AmfiProvider::extract_scheme_code(&inst).unwrap(), "120503");

        let invalid = ProviderInstrument::EquitySymbol {
            symbol: "INFY.NS".into(),
        };
        assert!(AmfiProvider::extract_scheme_code(&invalid).is_err());
    }

    #[test]
    fn test_parse_nav_date() {
        let dt = AmfiProvider::parse_nav_date("16-09-2026").unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2026-09-16");
    }
}
