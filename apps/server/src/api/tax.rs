use std::sync::Arc;

use crate::{error::ApiResult, main_lib::AppState};
use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Datelike;
use wealthfolio_core::{
    accounts::AccountServiceTrait,
    exports::{format_records, ExportFileFormat},
    lots::{compute_indian_tax_summary, to_schedule_cg_rows, IndianTaxYearSummary},
    portfolios::AccountScope,
};

#[derive(serde::Deserialize)]
pub struct IndianTaxSummaryQuery {
    #[serde(rename = "accountId")]
    pub account_id: Option<String>,
    #[serde(rename = "financialYear")]
    pub financial_year: Option<i32>,
}

#[derive(serde::Deserialize)]
pub struct IndianTaxSummaryBody {
    pub filter: Option<AccountScope>,
    #[serde(rename = "financialYear")]
    pub financial_year: Option<i32>,
}

fn resolve_tax_accounts(
    state: &AppState,
    filter: Option<&AccountScope>,
    single_account_id: Option<&str>,
) -> ApiResult<Vec<String>> {
    if let Some(account_id) = single_account_id {
        return Ok(vec![account_id.to_string()]);
    }

    match filter {
        None => {
            let accounts = state.account_service.get_active_accounts()?;
            Ok(accounts.into_iter().map(|a| a.id).collect())
        }
        Some(scope) => {
            let base = state.base_currency.read().unwrap().clone();
            let resolved = state
                .portfolio_service
                .resolve_account_scope(scope, &base)
                .map_err(crate::error::ApiError::from)?;
            Ok(resolved.account_ids)
        }
    }
}

fn default_financial_year() -> i32 {
    let now = chrono::Utc::now();
    if now.month() >= 4 {
        now.year()
    } else {
        now.year() - 1
    }
}

/// GET /tax/indian/summary?accountId=...&financialYear=...
async fn get_indian_tax_summary_get(
    State(state): State<Arc<AppState>>,
    Query(q): Query<IndianTaxSummaryQuery>,
) -> ApiResult<Json<IndianTaxYearSummary>> {
    let account_ids = resolve_tax_accounts(&state, None, q.account_id.as_deref())?;
    if account_ids.is_empty() {
        return Ok(Json(IndianTaxYearSummary::default()));
    }

    let fy = q.financial_year.unwrap_or_else(default_financial_year);
    let summary = compute_indian_tax_summary(
        state.lots_repository.as_ref(),
        state.asset_service.as_ref(),
        &account_ids,
        fy,
    )
    .await?;

    Ok(Json(summary))
}

/// POST /tax/indian/summary/query
async fn get_indian_tax_summary_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<IndianTaxSummaryBody>,
) -> ApiResult<Json<IndianTaxYearSummary>> {
    let account_ids = resolve_tax_accounts(&state, body.filter.as_ref(), None)?;
    if account_ids.is_empty() {
        return Ok(Json(IndianTaxYearSummary::default()));
    }

    let fy = body.financial_year.unwrap_or_else(default_financial_year);
    let summary = compute_indian_tax_summary(
        state.lots_repository.as_ref(),
        state.asset_service.as_ref(),
        &account_ids,
        fy,
    )
    .await?;

    Ok(Json(summary))
}

/// GET /tax/indian/schedule-cg/export?financialYear=...
async fn export_schedule_cg(
    State(state): State<Arc<AppState>>,
    Query(q): Query<IndianTaxSummaryQuery>,
) -> ApiResult<axum::response::Response> {
    use axum::http::header;
    use axum::response::IntoResponse;

    let account_ids = resolve_tax_accounts(&state, None, q.account_id.as_deref())?;
    let fy = q.financial_year.unwrap_or_else(default_financial_year);

    let summary = compute_indian_tax_summary(
        state.lots_repository.as_ref(),
        state.asset_service.as_ref(),
        &account_ids,
        fy,
    )
    .await?;

    let rows = to_schedule_cg_rows(&summary);
    let content = format_records(&rows, ExportFileFormat::Csv)
        .map_err(crate::error::ApiError::from)?
        .unwrap_or_default();

    let filename = format!("schedule_cg_fy{}_{}.csv", fy, fy + 1);

    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        content,
    )
        .into_response())
}

/// POST /tax/indian/schedule-cg/export/query
async fn export_schedule_cg_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<IndianTaxSummaryBody>,
) -> ApiResult<axum::response::Response> {
    use axum::http::header;
    use axum::response::IntoResponse;

    let account_ids = resolve_tax_accounts(&state, body.filter.as_ref(), None)?;
    let fy = body.financial_year.unwrap_or_else(default_financial_year);

    let summary = compute_indian_tax_summary(
        state.lots_repository.as_ref(),
        state.asset_service.as_ref(),
        &account_ids,
        fy,
    )
    .await?;

    let rows = to_schedule_cg_rows(&summary);
    let content = format_records(&rows, ExportFileFormat::Csv)
        .map_err(crate::error::ApiError::from)?
        .unwrap_or_default();

    let filename = format!("schedule_cg_fy{}_{}.csv", fy, fy + 1);

    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        content,
    )
        .into_response())
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tax/indian/summary", get(get_indian_tax_summary_get))
        .route("/tax/indian/summary/query", post(get_indian_tax_summary_post))
        .route("/tax/indian/schedule-cg/export", get(export_schedule_cg))
        .route("/tax/indian/schedule-cg/export/query", post(export_schedule_cg_post))
}
