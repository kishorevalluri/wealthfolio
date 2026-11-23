-- Back to plain FIFO for INR accounts on the India default.
UPDATE accounts
SET meta = json_set(meta, '$.accounting.costBasisMethod', 'FIFO')
WHERE upper(trim(currency)) = 'INR'
  AND meta IS NOT NULL
  AND json_valid(meta)
  AND json_extract(meta, '$.accounting.costBasisMethod') = 'FIFO_SAME_DAY';
