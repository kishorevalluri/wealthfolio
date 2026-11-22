-- The CAMS / KFintech CAS parser now writes fixed labels to "Transaction
-- Description" (PURCHASE, REDEMPTION, SWITCH IN, SWITCH OUT, DIVIDEND,
-- REINVESTMENT), stamp duty to "Fee", STT to "Tax" and the statement's own
-- description to "Comment".
UPDATE import_templates
SET config = '{"fieldMappings":{"date":"Date","activityType":"Transaction Description","symbol":"Scheme Name","isin":"ISIN","quantity":"Units","unitPrice":"NAV","amount":"Amount","account":"Folio No","fee":"Fee","tax":"Tax","comment":"Comment"},"activityMappings":{"BUY":["PURCHASE","SWITCH IN","REINVESTMENT"],"SELL":["REDEMPTION","SWITCH OUT"],"DIVIDEND":["DIVIDEND"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"auto","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    updated_at = CURRENT_TIMESTAMP
WHERE id = 'system_cams_cas';
