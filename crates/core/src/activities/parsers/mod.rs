//! Statement and tradebook parsers module.
//!
//! Provides independent, modular parsers for specific statement formats:
//! - CDSL e-CAS (`cdsl_cas`)
//! - CAMS / KFintech CAS (`cams_kfin_cas`)
//! - NSDL e-CAS (`nsdl_cas`)
//! - Zerodha Kite (`zerodha_kite`)
//! - Generic table fallback (`generic_pdf`)

pub mod cams_kfin_cas;
pub mod cdsl_cas;
pub mod generic_pdf;
pub mod nsdl_cas;
pub mod zerodha_kite;

pub use cams_kfin_cas::parse_cams_kfin_cas;
pub use cdsl_cas::parse_cdsl_cas;
pub use generic_pdf::parse_generic_pdf_table;
pub use nsdl_cas::parse_nsdl_cas;
pub use zerodha_kite::parse_zerodha_tradebook;
