-- Seed Indian Broker & Statement Templates: Zerodha Kite, Groww, and CAMS/KFintech CAS
-- All templates use scope='SYSTEM', kind='CSV_ACTIVITY', source_system=''

-- 1. Zerodha Kite Tradebook export
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_zerodha',
    'Zerodha Kite',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"trade_date","activityType":"trade_type","symbol":"symbol","isin":"isin","quantity":"quantity","unitPrice":"price","comment":"trade_id","instrumentType":"segment"},"activityMappings":{"BUY":["buy"],"SELL":["sell"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"yyyy-MM-dd","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);

-- 2. Groww Tradebook export
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_groww',
    'Groww',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Execution Date","activityType":"Type","symbol":"Symbol","isin":"ISIN","quantity":"Quantity","unitPrice":"Price","amount":"Order Value","comment":"Order Status"},"activityMappings":{"BUY":["BUY"],"SELL":["SELL"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"yyyy-MM-dd","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);

-- 3. CAMS / KFintech Consolidated Account Statement (CAS) export
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_cams_cas',
    'CAMS / KFintech CAS',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Date","activityType":"Transaction Description","symbol":"Scheme Name","isin":"ISIN","quantity":"Units","unitPrice":"NAV","amount":"Amount","account":"Folio No"},"activityMappings":{"BUY":["PURCHASE","SIP","ADDITIONAL PURCHASE","SWITCH IN","SWITCH_IN","SYSTEMATIC INVESTMENT","REINVESTMENT","REINVEST SHARES","DRIP"],"SELL":["REDEMPTION","SWITCH OUT","SWITCH_OUT","SWP","SYSTEMATIC WITHDRAWAL"],"DIVIDEND":["DIVIDEND PAYOUT","DIVIDEND REINVESTMENT","DIVIDEND","DIV"],"TAX":["STAMP DUTY","STAMP_DUTY","STT","SECURITIES TRANSACTION TAX","TDS","WITHHOLDING TAX"],"FEE":["BROKERAGE","TRANSACTION CHARGES","CHARGES"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"auto","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);
