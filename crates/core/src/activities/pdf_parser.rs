//! PDF parsing module for financial statements.
//!
//! Provides support for parsing password-protected and unencrypted PDF statements,
//! specifically Indian CAS statements (CAMS / KFintech mutual fund CAS, CDSL e-CAS, NSDL e-CAS).

use lopdf::Document;
use serde::{Deserialize, Serialize};

use crate::activities::csv_parser::{ParseConfig, ParsedCsvResult};
use crate::errors::{Error, ValidationError};
use crate::Result;

/// Type of detected Indian CAS statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StatementType {
    CamsCas,
    KfintechCas,
    CdslCas,
    NsdlCas,
    /// Upstox tradebook (.xlsx).
    UpstoxTradebook,
    /// A broker's contract note (Zerodha, Upstox, Dhan).
    ContractNote,
    Unknown,
}

impl StatementType {
    pub fn default_template_id(&self) -> Option<&'static str> {
        match self {
            StatementType::CamsCas | StatementType::KfintechCas => Some("system_cams_cas"),
            StatementType::CdslCas => Some("system_cdsl_cas"),
            StatementType::NsdlCas => Some("system_nsdl_cas"),
            StatementType::UpstoxTradebook => Some(super::spreadsheet_parser::UPSTOX_TEMPLATE_ID),
            StatementType::ContractNote => Some(CONTRACT_NOTE_TEMPLATE_ID),
            StatementType::Unknown => None,
        }
    }
}

/// Import template for broker contract notes.
pub const CONTRACT_NOTE_TEMPLATE_ID: &str = "system_contract_note";

/// Result of parsing a PDF statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedPdfResult {
    /// Extracted tabular result compatible with CSV parsing result.
    pub parsed: ParsedCsvResult,
    /// Statement type detected from document text.
    pub statement_type: StatementType,
    /// Recommended template ID to auto-select in the wizard.
    pub recommended_template_id: Option<String>,
}

/// Helper to check if a PDF buffer appears encrypted.
pub fn is_pdf_encrypted(bytes: &[u8]) -> bool {
    // Quick heuristic: check if /Encrypt dictionary exists in PDF trailer / xref
    if let Ok(doc) = Document::load_mem(bytes) {
        doc.is_encrypted()
    } else {
        // If it failed to load, check raw bytes for /Encrypt marker
        bytes.windows(8).any(|window| window == b"/Encrypt")
    }
}

/// Helper to parse Adobe ToUnicode CMaps (bfchar and bfrange).
fn parse_to_unicode_cmap(content: &[u8]) -> std::collections::HashMap<u32, String> {
    let mut map = std::collections::HashMap::new();
    let text = String::from_utf8_lossy(content);

    let mut in_bfchar = false;
    let mut in_bfrange = false;

    for line in text.lines() {
        let l = line.trim();
        if l.contains("beginbfchar") {
            in_bfchar = true;
            continue;
        }
        if l.contains("endbfchar") {
            in_bfchar = false;
            continue;
        }
        if l.contains("beginbfrange") {
            in_bfrange = true;
            continue;
        }
        if l.contains("endbfrange") {
            in_bfrange = false;
            continue;
        }

        if in_bfchar {
            let parts: Vec<&str> = l
                .split('>')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if parts.len() >= 2 {
                if let (Some(src_hex), Some(dst_hex)) =
                    (parts[0].strip_prefix('<'), parts[1].strip_prefix('<'))
                {
                    if let Ok(src_code) = u32::from_str_radix(src_hex.trim(), 16) {
                        let dst_hex_clean = dst_hex.trim();
                        let mut u16_chars = Vec::new();
                        for chunk in dst_hex_clean.as_bytes().chunks(4) {
                            if let Ok(chunk_str) = std::str::from_utf8(chunk) {
                                if let Ok(cp) = u16::from_str_radix(chunk_str, 16) {
                                    u16_chars.push(cp);
                                }
                            }
                        }
                        if let Ok(decoded_str) = String::from_utf16(&u16_chars) {
                            map.insert(src_code, decoded_str);
                        }
                    }
                }
            }
        } else if in_bfrange {
            let parts: Vec<&str> = l
                .split('>')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if parts.len() >= 3 {
                if let (Some(s_hex), Some(e_hex), Some(dst_hex)) = (
                    parts[0].strip_prefix('<'),
                    parts[1].strip_prefix('<'),
                    parts[2].strip_prefix('<'),
                ) {
                    if let (Ok(start), Ok(end), Ok(dst_start)) = (
                        u32::from_str_radix(s_hex.trim(), 16),
                        u32::from_str_radix(e_hex.trim(), 16),
                        u32::from_str_radix(dst_hex.trim(), 16),
                    ) {
                        for code in start..=end {
                            let offset = code - start;
                            if let Some(c) = char::from_u32(dst_start + offset) {
                                map.insert(code, c.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

/// Extract text from a lopdf Document, recursing into Form XObjects and decoding Identity-H / ToUnicode fonts.
/// A 2-D affine transform `[a b c d e f]`, as PDF writes it.
type Matrix = [f64; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m` then `n`: a point maps by `m` first.
fn multiply(m: &Matrix, n: &Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn operand_f64(operands: &[lopdf::Object], index: usize) -> f64 {
    match operands.get(index) {
        Some(lopdf::Object::Integer(i)) => *i as f64,
        Some(lopdf::Object::Real(r)) => f64::from(*r),
        _ => 0.0,
    }
}

fn shown_text(encoding: &lopdf::Encoding, operands: &[lopdf::Object], out: &mut String) {
    for operand in operands {
        match operand {
            lopdf::Object::String(bytes, _) => {
                if let Ok(text) = Document::decode_text(encoding, bytes) {
                    out.push_str(&text);
                }
            }
            lopdf::Object::Array(items) => shown_text(encoding, items, out),
            lopdf::Object::Integer(i) if *i < -100 => out.push(' '),
            lopdf::Object::Real(r) if *r < -100.0 => out.push(' '),
            _ => {}
        }
    }
}

/// Text of each page in reading order: every piece of text where the page
/// draws it, grouped into lines from the top of the page down and left to
/// right within a line. Some statements (KFintech's CAS) draw a page's
/// transaction table before the headings above it, so the order the content
/// stream gives does not say which scheme a row belongs to.
fn extract_positioned_text(doc: &Document) -> String {
    let mut out = String::new();
    for &page_id in doc.get_pages().values() {
        let encodings: std::collections::BTreeMap<Vec<u8>, lopdf::Encoding> = doc
            .get_page_fonts(page_id)
            .map(|fonts| {
                fonts
                    .into_iter()
                    .filter_map(|(name, font)| font.get_font_encoding(doc).ok().map(|e| (name, e)))
                    .collect()
            })
            .unwrap_or_default();
        let Ok(content) = lopdf::content::Content::decode(&doc.get_page_content(page_id)) else {
            continue;
        };
        let mut pieces: Vec<(f64, f64, String)> = Vec::new();
        let mut ctm = IDENTITY;
        let mut stack: Vec<Matrix> = Vec::new();
        let mut tm = IDENTITY;
        let mut tlm = IDENTITY;
        let mut leading = 0.0;
        let mut font: Option<Vec<u8>> = None;
        for op in &content.operations {
            let ops = &op.operands;
            match op.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(IDENTITY),
                "cm" => {
                    let m = [
                        operand_f64(ops, 0),
                        operand_f64(ops, 1),
                        operand_f64(ops, 2),
                        operand_f64(ops, 3),
                        operand_f64(ops, 4),
                        operand_f64(ops, 5),
                    ];
                    ctm = multiply(&m, &ctm);
                }
                "BT" => {
                    tm = IDENTITY;
                    tlm = IDENTITY;
                }
                "Tm" => {
                    tlm = [
                        operand_f64(ops, 0),
                        operand_f64(ops, 1),
                        operand_f64(ops, 2),
                        operand_f64(ops, 3),
                        operand_f64(ops, 4),
                        operand_f64(ops, 5),
                    ];
                    tm = tlm;
                }
                "Td" | "TD" => {
                    let (tx, ty) = (operand_f64(ops, 0), operand_f64(ops, 1));
                    if op.operator == "TD" {
                        leading = -ty;
                    }
                    tlm = multiply(&[1.0, 0.0, 0.0, 1.0, tx, ty], &tlm);
                    tm = tlm;
                }
                "TL" => leading = operand_f64(ops, 0),
                "T*" => {
                    tlm = multiply(&[1.0, 0.0, 0.0, 1.0, 0.0, -leading], &tlm);
                    tm = tlm;
                }
                "Tf" => {
                    font = ops
                        .first()
                        .and_then(|o| o.as_name().ok())
                        .map(<[u8]>::to_vec)
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    if op.operator != "Tj" && op.operator != "TJ" {
                        tlm = multiply(&[1.0, 0.0, 0.0, 1.0, 0.0, -leading], &tlm);
                        tm = tlm;
                    }
                    let Some(encoding) = font.as_ref().and_then(|f| encodings.get(f)) else {
                        continue;
                    };
                    let shown = if op.operator == "\"" {
                        ops.get(2).map(std::slice::from_ref).unwrap_or(&[])
                    } else {
                        ops.as_slice()
                    };
                    let mut text = String::new();
                    shown_text(encoding, shown, &mut text);
                    if text.trim().is_empty() {
                        continue;
                    }
                    let at = multiply(&tm, &ctm);
                    pieces.push((at[5], at[4], text));
                }
                _ => {}
            }
        }
        // Top of the page first, then left to right.
        pieces.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
        let mut lines: Vec<(f64, Vec<(f64, String)>)> = Vec::new();
        for (y, x, text) in pieces {
            match lines.last_mut() {
                Some((line_y, cells)) if (*line_y - y).abs() <= 2.5 => cells.push((x, text)),
                _ => lines.push((y, vec![(x, text)])),
            }
        }
        for (_, mut cells) in lines {
            cells.sort_by(|a, b| a.0.total_cmp(&b.0));
            let line: Vec<String> = cells
                .into_iter()
                .map(|(_, t)| t.trim().to_string())
                .collect();
            out.push_str(&line.join(" "));
            out.push('\n');
        }
    }
    out
}

fn extract_text_from_document(doc: &Document) -> String {
    let mut full_text = String::new();
    let pages = doc.get_pages();

    for (page_num, &page_id) in pages.iter() {
        let mut page_text = String::new();

        // 1. Try built-in extract_text for standard content streams
        if let Ok(t) = doc.extract_text(&[*page_num]) {
            if !t.trim().is_empty() {
                page_text.push_str(&t);
                if !page_text.ends_with('\n') {
                    page_text.push('\n');
                }
            }
        }

        // 2. Extract from Form XObjects (crucial for iText / modern Indian CAS statements like CDSL)
        // Check page resources for XObjects
        let mut form_xobject_ids = Vec::new();
        if let Ok((res_dict_opt, res_ids)) = doc.get_page_resources(page_id) {
            let mut all_res_dicts = Vec::new();
            if let Some(d) = res_dict_opt {
                all_res_dicts.push(d);
            }
            for rid in res_ids {
                if let Ok(d) = doc.get_dictionary(rid) {
                    all_res_dicts.push(d);
                }
            }

            for resources_dict in all_res_dicts {
                if let Ok(xobjects) = resources_dict.get(b"XObject").and_then(|o| o.as_dict()) {
                    for (_name, obj) in xobjects.iter() {
                        if let Ok(xobj_id) = obj.as_reference() {
                            if let Ok(stream) = doc.get_object(xobj_id).and_then(|o| o.as_stream())
                            {
                                if let Ok(subtype) =
                                    stream.dict.get(b"Subtype").and_then(|o| o.as_name())
                                {
                                    if subtype == b"Form" {
                                        form_xobject_ids.push(xobj_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        for xobj_id in form_xobject_ids {
            if let Ok(xobj_stream) = doc.get_object(xobj_id).and_then(|o| o.as_stream()) {
                // Get fonts for this Form XObject
                let mut font_cmaps: std::collections::HashMap<
                    Vec<u8>,
                    std::collections::HashMap<u32, String>,
                > = std::collections::HashMap::new();

                let res_dict = if let Ok(res_id) = xobj_stream
                    .dict
                    .get(b"Resources")
                    .and_then(|o| o.as_reference())
                {
                    doc.get_dictionary(res_id).ok()
                } else {
                    xobj_stream
                        .dict
                        .get(b"Resources")
                        .and_then(|o| o.as_dict())
                        .ok()
                };

                if let Some(res_dict) = res_dict {
                    if let Ok(font_dict) = res_dict.get(b"Font").and_then(|o| o.as_dict()) {
                        for (font_name, font_ref) in font_dict.iter() {
                            if let Ok(f_id) = font_ref.as_reference() {
                                if let Ok(f_dict) = doc.get_dictionary(f_id) {
                                    if let Ok(to_unicode_id) =
                                        f_dict.get(b"ToUnicode").and_then(|o| o.as_reference())
                                    {
                                        if let Ok(tu_stream) = doc
                                            .get_object(to_unicode_id)
                                            .and_then(|o| o.as_stream())
                                        {
                                            let mut tu_s = tu_stream.clone();
                                            let _ = tu_s.decompress();
                                            let cmap = parse_to_unicode_cmap(&tu_s.content);
                                            font_cmaps.insert(font_name.clone(), cmap);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let mut s = xobj_stream.clone();
                let _ = s.decompress();
                if let Ok(form_content) = lopdf::content::Content::decode(&s.content) {
                    let mut current_font: Option<Vec<u8>> = None;
                    let empty_map = std::collections::HashMap::new();

                    for op in form_content.operations.iter() {
                        match op.operator.as_str() {
                            "Tf" => {
                                if let Some(font_name) =
                                    op.operands.first().and_then(|o| o.as_name().ok())
                                {
                                    current_font = Some(font_name.to_vec());
                                }
                            }
                            "TJ" => {
                                let cmap = current_font
                                    .as_ref()
                                    .and_then(|f| font_cmaps.get(f))
                                    .unwrap_or(&empty_map);
                                if let Some(lopdf::Object::Array(arr)) = op.operands.first() {
                                    for item in arr {
                                        match item {
                                            lopdf::Object::String(bytes, _) => {
                                                if !cmap.is_empty() {
                                                    for chunk in bytes.chunks(2) {
                                                        let cid = if chunk.len() == 2 {
                                                            ((chunk[0] as u32) << 8)
                                                                | (chunk[1] as u32)
                                                        } else {
                                                            chunk[0] as u32
                                                        };
                                                        if let Some(ch) = cmap.get(&cid) {
                                                            page_text.push_str(ch);
                                                        }
                                                    }
                                                } else {
                                                    page_text
                                                        .push_str(&String::from_utf8_lossy(bytes));
                                                }
                                            }
                                            lopdf::Object::Integer(i) if *i < -100 => {
                                                page_text.push(' ');
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            "Tj" => {
                                let cmap = current_font
                                    .as_ref()
                                    .and_then(|f| font_cmaps.get(f))
                                    .unwrap_or(&empty_map);
                                if let Some(lopdf::Object::String(bytes, _)) = op.operands.first() {
                                    if !cmap.is_empty() {
                                        for chunk in bytes.chunks(2) {
                                            let cid = if chunk.len() == 2 {
                                                ((chunk[0] as u32) << 8) | (chunk[1] as u32)
                                            } else {
                                                chunk[0] as u32
                                            };
                                            if let Some(ch) = cmap.get(&cid) {
                                                page_text.push_str(ch);
                                            }
                                        }
                                    } else {
                                        page_text.push_str(&String::from_utf8_lossy(bytes));
                                    }
                                }
                            }
                            "ET" | "T*" if !page_text.ends_with('\n') => {
                                page_text.push('\n');
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if !page_text.trim().is_empty() {
            full_text.push_str(&page_text);
            if !full_text.ends_with('\n') {
                full_text.push('\n');
            }
        }
    }

    full_text
}

/// Parse a PDF document into a tabular format.
pub fn parse_pdf(content: &[u8], password: Option<&str>) -> Result<ParsedPdfResult> {
    // Broker reports also come as spreadsheets; they share this entry point.
    if super::spreadsheet_parser::looks_like_spreadsheet(content) {
        return super::spreadsheet_parser::parse_spreadsheet(content);
    }
    let pwd = password.unwrap_or("").trim();

    // 1. Try loading with password first if provided
    let doc = if !pwd.is_empty() {
        let loaded =
            Document::load_mem_with_options(content, lopdf::LoadOptions::with_password(pwd))
                .ok()
                .or_else(|| {
                    if pwd.chars().any(|c| c.is_ascii_lowercase()) {
                        let upper = pwd.to_uppercase();
                        Document::load_mem_with_options(
                            content,
                            lopdf::LoadOptions::with_password(&upper),
                        )
                        .ok()
                    } else {
                        None
                    }
                });

        match loaded {
            Some(d) => d,
            None => {
                // If load_mem_with_options failed, attempt standard load_mem + decrypt
                let mut d = Document::load_mem(content).map_err(|e| {
                    Error::Validation(ValidationError::InvalidInput(format!(
                        "Failed to open PDF: {}",
                        e
                    )))
                })?;
                if d.is_encrypted() {
                    let decrypt_res = d.decrypt(pwd).or_else(|orig_err| {
                        if pwd.chars().any(|c| c.is_ascii_lowercase()) {
                            let upper = pwd.to_uppercase();
                            d.decrypt(&upper).map_err(|_| orig_err)
                        } else {
                            Err(orig_err)
                        }
                    });
                    decrypt_res.map_err(|e| {
                        Error::Validation(ValidationError::InvalidInput(format!(
                            "Failed to decrypt PDF (wrong password or unsupported encryption: {})",
                            e
                        )))
                    })?;
                }
                d
            }
        }
    } else {
        // No password provided: check if the PDF is encrypted first!
        if is_pdf_encrypted(content) {
            return Err(Error::Validation(ValidationError::InvalidInput(
                "PDF is password-protected. Please provide a password.".to_string(),
            )));
        }

        let d = Document::load_mem(content).map_err(|e| {
            Error::Validation(ValidationError::InvalidInput(format!(
                "Failed to open PDF: {}",
                e
            )))
        })?;

        if d.is_encrypted() {
            return Err(Error::Validation(ValidationError::InvalidInput(
                "PDF is password-protected. Please provide a password.".to_string(),
            )));
        }

        d
    };

    // Extract text from all pages and Form XObjects
    let full_text = extract_text_from_document(&doc);

    if full_text.trim().is_empty() {
        return Err(Error::Validation(ValidationError::InvalidInput(
            "Could not extract readable text from PDF. The PDF may be scanned/image-based."
                .to_string(),
        )));
    }

    let statement_type = detect_statement_type(&full_text);
    // CAMS / KFintech statements are read in page order, by position.
    let full_text = match statement_type {
        StatementType::CamsCas | StatementType::KfintechCas => {
            let positioned = extract_positioned_text(&doc);
            if positioned.trim().is_empty() {
                full_text
            } else {
                positioned
            }
        }
        _ => full_text,
    };

    let (headers, rows) = match statement_type {
        StatementType::CamsCas | StatementType::KfintechCas => {
            crate::activities::parsers::parse_cams_kfin_cas(&full_text)
        }
        StatementType::CdslCas => crate::activities::parsers::parse_cdsl_cas(&full_text),
        StatementType::NsdlCas => crate::activities::parsers::parse_nsdl_cas(&full_text),
        StatementType::ContractNote => crate::activities::parsers::parse_contract_note(&full_text),
        StatementType::UpstoxTradebook | StatementType::Unknown => {
            crate::activities::parsers::parse_generic_pdf_table(&full_text)
        }
    };

    let recommended_template_id = statement_type.default_template_id().map(|s| s.to_string());

    let row_count = rows.len();
    Ok(ParsedPdfResult {
        parsed: ParsedCsvResult {
            headers,
            rows,
            detected_config: ParseConfig {
                delimiter: Some(",".to_string()),
                date_format: Some("auto".to_string()),
                decimal_separator: Some(".".to_string()),
                thousands_separator: Some(",".to_string()),
                has_header_row: Some(true),
                ..Default::default()
            },
            errors: Vec::new(),
            row_count,
        },
        statement_type,
        recommended_template_id,
    })
}

/// Detect the Indian statement type from the extracted text.
fn detect_statement_type(text: &str) -> StatementType {
    let lower = text.to_lowercase();

    // A contract note can name its depository (Dhan's says CDSL), so it is
    // recognised before the depository statements.
    if crate::activities::parsers::is_contract_note(text) {
        return StatementType::ContractNote;
    }

    // A CAMS / KFintech CAS names the depositories too (demat folios), but
    // only it lists each scheme's opening unit balance.
    if crate::activities::parsers::is_cams_kfin_cas(text) {
        return if lower.contains("cams") {
            StatementType::CamsCas
        } else {
            StatementType::KfintechCas
        };
    }

    // CDSL e-CAS often lists MF holdings with "RTA : CAMS" or "RTA : KFINTECH",
    // so CDSL and NSDL headers must take precedence over CAMS/KFintech!
    if lower.contains("central depository services")
        || lower.contains("cdsl")
        || lower.contains("bo id")
        || (lower.contains("e-cas") && lower.contains("cdsl"))
    {
        return StatementType::CdslCas;
    }

    if lower.contains("national securities depository")
        || lower.contains("nsdl")
        || (lower.contains("client id") && lower.contains("depository"))
    {
        return StatementType::NsdlCas;
    }

    if lower.contains("cams")
        && (lower.contains("consolidated account statement")
            || lower.contains("folio no")
            || lower.contains("camsonline"))
    {
        return StatementType::CamsCas;
    }

    if lower.contains("kfintech")
        || lower.contains("karvy")
        || (lower.contains("consolidated account statement") && lower.contains("kfin technologies"))
    {
        return StatementType::KfintechCas;
    }

    // Secondary check if "Consolidated Account Statement" with MF terms
    if lower.contains("consolidated account statement") && lower.contains("nav") {
        return StatementType::CamsCas;
    }

    StatementType::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_statement_type() {
        let cams_text = "Consolidated Account Statement
CAMS Online Services
Folio No: 123456/78
NAV: 45.2";
        assert_eq!(detect_statement_type(cams_text), StatementType::CamsCas);

        let cdsl_text = "Central Depository Services (India) Limited
BO ID: 1208160012345678
e-CAS Statement";
        assert_eq!(detect_statement_type(cdsl_text), StatementType::CdslCas);

        let nsdl_text = "National Securities Depository Limited
Client ID: 10023456
Transaction Statement";
        assert_eq!(detect_statement_type(nsdl_text), StatementType::NsdlCas);

        // Dhan's contract notes name its CDSL depository participant.
        let note_text = "Contract Note No: 4300001 Trade Date: 18-05-2026
Depository Participant of CDSL.: IN-DP-289-2016";
        assert_eq!(
            detect_statement_type(note_text),
            StatementType::ContractNote
        );
        assert_eq!(
            StatementType::ContractNote.default_template_id(),
            Some(CONTRACT_NOTE_TEMPLATE_ID)
        );
    }
}
