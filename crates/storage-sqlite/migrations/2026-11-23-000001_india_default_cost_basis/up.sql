-- Indian (INR) accounts now default to FIFO_SAME_DAY: a sale on a day with a
-- purchase is matched with that purchase first, as Indian tax treats it as an
-- intraday trade. Accounts created before this stored the old default (FIFO)
-- explicitly, so move those that still hold FIFO, or no method, to the new
-- default. The accounts trigger marks each changed account for recalculation.
UPDATE accounts
SET meta = json_set(
        json_remove(meta, '$.accounting.cost_basis_method'),
        '$.accounting.costBasisMethod',
        'FIFO_SAME_DAY'
    )
WHERE upper(trim(currency)) = 'INR'
  AND meta IS NOT NULL
  AND json_valid(meta)
  AND json_type(meta) = 'object'
  AND (json_type(meta, '$.accounting') IS NULL OR json_type(meta, '$.accounting') = 'object')
  AND upper(coalesce(
        json_extract(meta, '$.accounting.costBasisMethod'),
        json_extract(meta, '$.accounting.cost_basis_method'),
        'FIFO'
      )) = 'FIFO';

UPDATE accounts
SET meta = json_object('accounting', json_object('costBasisMethod', 'FIFO_SAME_DAY'))
WHERE upper(trim(currency)) = 'INR'
  AND (meta IS NULL OR trim(meta) = '');
