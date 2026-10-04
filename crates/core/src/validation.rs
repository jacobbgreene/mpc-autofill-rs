//! Limits and identifiers shared by every untrusted import path.
use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;

use crate::{Error, Result};

pub const MAX_XML_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FIELD_BYTES: usize = 16 * 1024;
pub const MAX_CARD_RECORDS: usize = 2 * crate::order::PROJECT_MAX_SIZE as usize;
const MAX_XML_DEPTH: usize = 16;
const MAX_XML_ELEMENTS: usize = 20_000;
const MAX_ATTRIBUTES_PER_ELEMENT: usize = 8;
const MAX_ATTRIBUTES: usize = 512;

pub fn validate_drive_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(Error::Invalid(
            "artwork has an invalid Google Drive ID".into(),
        ));
    }
    Ok(())
}

pub fn validate_field(value: &str, label: &str) -> Result<()> {
    if value.len() > MAX_FIELD_BYTES || value.contains('\0') {
        return Err(Error::Invalid(format!(
            "{label} is too long or contains a null character"
        )));
    }
    Ok(())
}

/// Bound the actual read, even if a file grows after its metadata was checked.
/// This synchronous helper belongs on a blocking worker when used from async UI code.
pub fn read_text_file_bounded(path: impl AsRef<Path>, max_bytes: usize) -> Result<String> {
    let path = path.as_ref();
    // Reject directories and special files before opening (a FIFO open can block).
    if !std::fs::metadata(path)?.is_file() {
        return Err(Error::Invalid("choose a regular text file".into()));
    }
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_bytes as u64 {
        return Err(Error::Invalid(format!(
            "choose a regular text file no larger than {max_bytes} bytes"
        )));
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(Error::Invalid(format!(
            "text file exceeds {max_bytes} bytes"
        )));
    }
    String::from_utf8(bytes).map_err(|_| Error::Invalid("text file is not valid UTF-8".into()))
}

/// Scan with the non-namespace reader before serde allocates records or resolves
/// namespaces. Every document and individual value has a finite work budget.
pub(crate) fn validate_xml(xml: &str) -> Result<()> {
    if xml.len() > MAX_XML_BYTES {
        return Err(Error::Invalid("order XML exceeds 8 MiB".into()));
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut field_sizes: Vec<(usize, bool)> = Vec::new();
    let mut roots = 0;
    let mut elements = 0;
    let mut cards = 0;
    let mut attributes = 0;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| Error::Invalid(format!("malformed XML: {error}")))?;
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                if field_sizes.len() >= MAX_XML_DEPTH {
                    return Err(Error::Invalid("order XML is nested too deeply".into()));
                }
                elements += 1;
                if elements > MAX_XML_ELEMENTS || tag.name().as_ref().len() > 128 {
                    return Err(Error::Invalid(
                        "order XML has too many elements or an oversized tag".into(),
                    ));
                }
                if field_sizes.is_empty() {
                    roots += 1;
                    if roots != 1 || tag.name().as_ref() != "order" {
                        return Err(Error::Invalid(
                            "order XML must contain one order root".into(),
                        ));
                    }
                } else if let Some((size, has_children)) = field_sizes.last_mut() {
                    // Pretty-print indentation in a container is not a field.
                    *size = 0;
                    *has_children = true;
                }
                if tag.local_name().as_ref() == "card" {
                    cards += 1;
                    if cards > MAX_CARD_RECORDS {
                        return Err(Error::Invalid(
                            "order XML has too many artwork records".into(),
                        ));
                    }
                }
                // Bound attribute work before duplicate checks. Only eight
                // keys can enter this small set, including namespace declarations.
                let mut keys = Vec::new();
                for (index, attribute) in tag.attributes().with_checks(false).enumerate() {
                    attributes += 1;
                    if index >= MAX_ATTRIBUTES_PER_ELEMENT || attributes > MAX_ATTRIBUTES {
                        return Err(Error::Invalid("order XML has too many attributes".into()));
                    }
                    let attribute = attribute.map_err(|error| {
                        Error::Invalid(format!("malformed XML attribute: {error}"))
                    })?;
                    if attribute.key.as_ref().len() > 128 || attribute.value.len() > MAX_FIELD_BYTES
                    {
                        return Err(Error::Invalid(
                            "order XML has an oversized attribute".into(),
                        ));
                    }
                    if keys.contains(&attribute.key) {
                        return Err(Error::Invalid("order XML repeats an attribute".into()));
                    }
                    keys.push(attribute.key);
                }
                if matches!(event, Event::Start(_)) {
                    field_sizes.push((0, false));
                }
            }
            Event::End(_) => {
                if field_sizes.pop().is_none() {
                    return Err(Error::Invalid(
                        "order XML has an unmatched closing tag".into(),
                    ));
                }
            }
            Event::Text(ref value) => count_text(&mut field_sizes, value.as_ref().as_bytes())?,
            Event::CData(ref value) => count_text(&mut field_sizes, value.as_ref().as_bytes())?,
            Event::GeneralRef(ref value) => {
                count_text(&mut field_sizes, value.as_ref().as_bytes())?
            }
            Event::DocType(_) => {
                return Err(Error::Invalid(
                    "order XML cannot contain a document type or entity declarations".into(),
                ))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if roots != 1 || !field_sizes.is_empty() {
        return Err(Error::Invalid("order XML is incomplete".into()));
    }
    Ok(())
}

fn count_text(fields: &mut [(usize, bool)], value: &[u8]) -> Result<()> {
    if let Some((size, has_children)) = fields.last_mut() {
        if *has_children && value.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        *size += value.len();
        if *size > MAX_FIELD_BYTES {
            return Err(Error::Invalid("order XML has an oversized field".into()));
        }
    } else if value.iter().any(|byte| !byte.is_ascii_whitespace()) {
        return Err(Error::Invalid(
            "order XML contains text outside its root".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_ids_have_one_contract_for_every_import_path() {
        for id in [
            "",
            "../card",
            r"..\card",
            "/tmp/card",
            r"C:\card",
            r"\\server\card",
            "art#/../../escape",
            "art?x=y",
            "a.b",
            "é",
            "a\0b",
        ] {
            assert!(validate_drive_id(id).is_err(), "{id:?}");
        }
        assert!(validate_drive_id(&"a".repeat(257)).is_err());
        assert!(validate_drive_id(&"a".repeat(256)).is_ok());
        assert!(validate_drive_id("art_1-2").is_ok());
    }

    #[test]
    fn bounded_reads_reject_oversized_files_and_directories() {
        let root = std::env::temp_dir().join(format!(
            "mpc-bounded-read-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("input.txt");
        std::fs::write(&path, "12345").unwrap();
        assert_eq!(read_text_file_bounded(&path, 5).unwrap(), "12345");
        assert!(read_text_file_bounded(&path, 4).is_err());
        assert!(read_text_file_bounded(&root, 100).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "12345");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
