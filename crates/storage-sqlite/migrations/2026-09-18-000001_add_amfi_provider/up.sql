-- Add AMFI market data provider for Indian mutual funds
INSERT OR IGNORE INTO market_data_providers (id, name, description, url, priority, enabled, logo_filename, last_synced_at, last_sync_status, last_sync_error, provider_type, config)
VALUES ('AMFI', 'AMFI (India)', 'Association of Mutual Funds in India via mfapi.in for NAV and mutual fund quotes.', 'https://www.amfiindia.com/', 5, TRUE, NULL, NULL, NULL, NULL, 'builtin', NULL);
