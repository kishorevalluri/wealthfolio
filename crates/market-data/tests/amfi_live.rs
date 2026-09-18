//! Live upstream integration test for AMFI mutual fund provider.
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use wealthfolio_market_data::{
    AmfiProvider, InstrumentId, MarketDataProvider, ProviderInstrument, QuoteContext,
    QuoteIdentifiers,
};

fn amfi_context(scheme_code: &str) -> QuoteContext {
    QuoteContext {
        instrument: InstrumentId::Equity {
            ticker: scheme_code.into(),
            mic: None,
        },
        identifiers: QuoteIdentifiers::default(),
        overrides: None,
        currency_hint: Some("INR".into()),
        preferred_provider: Some("AMFI".into()),
        bond_metadata: None,
        custom_provider_code: None,
    }
}

#[tokio::test]
async fn amfi_fetch_nav_and_history_for_scheme_120503() {
    let provider = AmfiProvider::new();
    let scheme_code = "120503";
    let ctx = amfi_context(scheme_code);
    let instrument = ProviderInstrument::EquitySymbol {
        symbol: scheme_code.into(),
    };

    // 1. Fetch latest quote (NAV)
    let latest = provider
        .get_latest_quote(&ctx, instrument.clone())
        .await
        .expect("Failed to fetch latest NAV for scheme 120503");

    assert!(
        latest.close > Decimal::ZERO,
        "NAV should be positive, got {}",
        latest.close
    );
    assert_eq!(latest.currency, "INR");
    assert_eq!(latest.source, "AMFI");

    // 2. Fetch historical quotes
    let end = Utc::now();
    let start = end - Duration::days(60);
    let history = provider
        .get_historical_quotes(&ctx, instrument, start, end)
        .await
        .expect("Failed to fetch historical quotes for scheme 120503");

    assert!(!history.is_empty(), "History should not be empty");
    assert!(
        history.iter().all(|q| q.close > Decimal::ZERO),
        "All historical NAVs should be positive"
    );

    // 3. Search for Nippon scheme
    let search_results = provider
        .search("Nippon")
        .await
        .expect("Search should succeed");
    assert!(
        !search_results.is_empty(),
        "Search for 'Nippon' should return results"
    );

    // 4. Fetch profile
    let profile = provider
        .get_profile(scheme_code)
        .await
        .expect("Failed to fetch profile for scheme 120503");
    assert!(profile.name.is_some());
    assert_eq!(profile.currency.as_deref(), Some("INR"));
}
