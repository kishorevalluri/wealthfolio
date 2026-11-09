-- CDSL e-CAS parser now writes fixed labels to "Transaction Description"
-- (BUY, SELL, TRANSFER IN, TRANSFER OUT, DIVIDEND, plus corporate action and
-- unknown credit/debit labels left for the user to map), stamp duty to "Fee"
-- and the statement text to "Particulars".
UPDATE import_templates
SET config = '{"fieldMappings":{"date":"Date","activityType":"Transaction Description","symbol":"Security Description","isin":"ISIN","quantity":"Quantity","unitPrice":"Rate","amount":"Amount","account":"BO ID","fee":"Fee","comment":"Particulars"},"activityMappings":{"BUY":["BUY"],"SELL":["SELL"],"TRANSFER_IN":["TRANSFER IN"],"TRANSFER_OUT":["TRANSFER OUT"],"DIVIDEND":["DIVIDEND"]},"symbolMappings":{},"accountMappings":{},"symbolMappingMeta":{},"parseConfig":{"delimiter":",","dateFormat":"auto","decimalSeparator":"auto","thousandsSeparator":"auto","hasHeaderRow":true,"skipTopRows":0,"skipBottomRows":0}}',
    updated_at = CURRENT_TIMESTAMP
WHERE id = 'system_cdsl_cas';
