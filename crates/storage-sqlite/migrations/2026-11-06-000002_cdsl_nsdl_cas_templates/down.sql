-- Revert CDSL and NSDL e-CAS templates
DELETE FROM import_templates WHERE id IN ('system_cdsl_cas', 'system_nsdl_cas');
