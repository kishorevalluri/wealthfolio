//! NSE's corporate actions, as its website's corporate filings page reads
//! them. NSE serves the data only to a browser-like client holding the cookies
//! its home page sets, so the client opens the home page first, and NSE may
//! still refuse (it rate-limits and blocks automated traffic).

use std::time::Duration;

use chrono::NaiveDate;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, REFERER};
use serde_json::Value;

use super::proposals::ExchangeAction;
use crate::errors::{Error, ValidationError};
use crate::Result;

pub const NSE_SOURCE: &str = "NSE";

const HOME: &str = "https://www.nseindia.com/";
const PAGE: &str = "https://www.nseindia.com/companies-listing/corporate-filings-actions";
const API: &str = "https://www.nseindia.com/api/corporates-corporateActions";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

pub struct NseClient {
    client: reqwest::Client,
    warmed: tokio::sync::OnceCell<()>,
}

fn unavailable(message: String) -> Error {
    Error::Validation(ValidationError::InvalidInput(message))
}

impl NseClient {
    pub fn new() -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| unavailable(format!("Could not set up the NSE client: {e}")))?;
        Ok(Self {
            client,
            warmed: tokio::sync::OnceCell::new(),
        })
    }

    async fn warm_up(&self) {
        self.warmed
            .get_or_init(|| async {
                // The cookies are what matters; the pages' status doesn't.
                for url in [HOME, PAGE] {
                    let _ = self
                        .client
                        .get(url)
                        .header(ACCEPT, "text/html,application/xhtml+xml")
                        .send()
                        .await;
                }
            })
            .await;
    }

    /// Every corporate action NSE lists for `symbol` (equity series).
    pub async fn actions_for(&self, symbol: &str) -> Result<Vec<ExchangeAction>> {
        self.warm_up().await;
        let response = self
            .client
            .get(API)
            .query(&[("index", "equities"), ("symbol", symbol)])
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(REFERER, PAGE)
            .send()
            .await
            .map_err(|e| unavailable(format!("NSE could not be reached: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(unavailable(format!(
                "NSE refused the request (HTTP {}). NSE blocks automated requests at times; try again later.",
                status.as_u16()
            )));
        }
        let body: Value = response.json().await.map_err(|_| {
            unavailable("NSE sent a reply that isn't corporate-action data.".into())
        })?;
        Ok(parse_nse_actions(&body, symbol))
    }
}

fn text<'a>(row: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| row.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "-")
}

fn date(value: Option<&str>) -> Option<NaiveDate> {
    let value = value?;
    ["%d-%b-%Y", "%d-%m-%Y", "%Y-%m-%d", "%d %b %Y"]
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(value, f).ok())
}

/// Rows of NSE's reply: an array, or an object holding one under `data`.
pub(crate) fn parse_nse_actions(body: &Value, symbol: &str) -> Vec<ExchangeAction> {
    let rows = body
        .as_array()
        .or_else(|| body.get("data").and_then(Value::as_array));
    let Some(rows) = rows else {
        return Vec::new();
    };
    rows.iter()
        .filter(|row| {
            // Equity series only; a row without a series is kept.
            text(row, &["series"]).is_none_or(|s| matches!(s, "EQ" | "BE" | "BZ" | "SM" | "ST"))
        })
        .filter_map(|row| {
            let subject = text(row, &["subject", "purpose"])?.to_string();
            let ex_date = date(text(row, &["exDate", "exdate", "ex_date"]))?;
            Some(ExchangeAction {
                symbol: text(row, &["symbol"])
                    .unwrap_or(symbol)
                    .to_ascii_uppercase(),
                isin: text(row, &["isin"]).map(str::to_string),
                company: text(row, &["comp", "company", "companyName"]).map(str::to_string),
                subject,
                ex_date,
                record_date: date(text(row, &["recDate", "recordDate", "record_date"])),
            })
        })
        .collect()
}
