-- Seed CDSL and NSDL e-CAS (Consolidated Account Statement) Templates
-- All templates use scope='SYSTEM', kind='CSV_ACTIVITY', source_system=''

-- 1. CDSL e-CAS (Central Depository Services Limited) transaction export
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_cdsl_cas',
    'CDSL e-CAS',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Date","activityType":"Transaction Description","symbol":"Security Description","isin":"ISIN","quantity":"Quantity","unitPrice":"Rate","amount":"Amount","account":"BO ID"},"activityMappings":{"BUY":["PURCHASE","BUY","MARKET BUY","OFF-MARKET INWARD","OFF MARKET INWARD","IPO ALLOTMENT","IPO","BONUS","RIGHTS","CORPORATE ACTION INWARD","INTER-DEPOSITORY TRANSFER INWARD","INWARD TRANSFER","TRANSMISSION INWARD"],"SELL":["SALE","SELL","MARKET SELL","OFF-MARKET OUTWARD","OFF MARKET OUTWARD","REDEMPTION","CORPORATE ACTION OUTWARD","INTER-DEPOSITORY TRANSFER OUTWARD","OUTWARD TRANSFER","TRANSMISSION OUTWARD"],"SPLIT":["STOCK SPLIT","SUBDIVISION"],"FEE":["CHARGES","DEPOSITORY CHARGES"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"auto","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);

-- 2. NSDL e-CAS (National Securities Depository Limited) transaction export
INSERT OR REPLACE INTO import_templates (id, name, scope, kind, source_system, config_version, config, created_at, updated_at)
VALUES (
    'system_nsdl_cas',
    'NSDL e-CAS',
    'SYSTEM',
    'CSV_ACTIVITY',
    '',
    1,
    '{"fieldMappings":{"date":"Transaction Date","activityType":"Particulars","symbol":"Security Name","isin":"ISIN","quantity":"Quantity","unitPrice":"Rate","amount":"Amount","account":"Client ID"},"activityMappings":{"BUY":["BUY","PURCHASE","MARKET BUY","OFF-MARKET INWARD","OFF MARKET INWARD","IPO ALLOTMENT","IPO","BONUS","RIGHTS","CORPORATE ACTION INWARD","INTER-DEPOSITORY TRANSFER INWARD","INWARD TRANSFER","DEMAT INWARD"],"SELL":["SELL","SALE","MARKET SELL","OFF-MARKET OUTWARD","OFF MARKET OUTWARD","REDEMPTION","CORPORATE ACTION OUTWARD","INTER-DEPOSITORY TRANSFER OUTWARD","OUTWARD TRANSFER","DEMAT OUTWARD"],"SPLIT":["SUB-DIVISION","STOCK SPLIT"],"FEE":["CHARGES","DEP CHARGES"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"auto","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
);
