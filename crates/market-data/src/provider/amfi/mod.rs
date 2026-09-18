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
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
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

#[derive(Debug, Deserialize)]
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

        let url = format!("{}/mf/search?q={}", BASE_URL, urlencoding::encode(trimmed));
        debug!("AMFI search: {}", url);

        let resp = self.send_get(&url).await?;
        if !resp.status().is_success() {
            return Err(MarketDataError::ProviderError {
                provider: PROVIDER_ID.to_string(),
                message: format!("HTTP {}", resp.status()),
            });
        }

        let items: Vec<MfSearchResultItem> =
            resp.json()
                .await
                .map_err(|e| MarketDataError::ProviderError {
                    provider: PROVIDER_ID.to_string(),
                    message: format!("JSON parse error: {}", e),
                })?;

        let results = items
            .into_iter()
            .map(|item| {
                let code_str = item.scheme_code.to_string();
                let score = if code_str.eq_ignore_ascii_case(trimmed) {
                    25000.0
                } else if item.scheme_name.to_lowercase().contains(&trimmed.to_lowercase()) {
                    20000.0
                } else {
                    15000.0
                };

                SearchResult::new(code_str, item.scheme_name, "AMFI", "MUTUALFUND")
                    .with_currency("INR")
                    .with_data_source(PROVIDER_ID)
                    .with_score(score)
            })
            .collect();

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

        let mut profile = AssetProfile {
            source: Some(PROVIDER_ID.to_string()),
            name: meta.scheme_name,
            quote_type: Some("MUTUALFUND".to_string()),
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

        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
