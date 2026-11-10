-- Upstox tradebook (.xlsx). The spreadsheet parser writes these columns; the
-- BSE scrip code becomes the symbol (e.g. 543983.BO) as the report has no ISIN.
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_upstox',
    'Upstox Tradebook',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Date","activityType":"Side","symbol":"Symbol","quantity":"Quantity","unitPrice":"Price","amount":"Amount","comment":"Trade Num"},"activityMappings":{"BUY":["Buy"],"SELL":["Sell"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"auto","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);
