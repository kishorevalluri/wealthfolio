-- Broker contract notes (Zerodha, Upstox, Dhan PDFs). The contract note parser
-- writes these columns: one row per security and side, with the note's
-- charges shared out by value, STT as the tax and the rest as the fee.
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_contract_note',
    'Broker Contract Note',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Date","activityType":"Side","symbol":"Symbol","isin":"ISIN","quantity":"Quantity","unitPrice":"Price","fee":"Fee","tax":"Tax","currency":"Currency","comment":"Comment"},"activityMappings":{"BUY":["BUY"],"SELL":["SELL"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"auto","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);
