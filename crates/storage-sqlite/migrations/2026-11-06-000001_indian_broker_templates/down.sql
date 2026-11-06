-- Revert Indian broker and CAS templates
DELETE FROM import_templates WHERE id IN ('system_zerodha', 'system_groww', 'system_cams_cas');
