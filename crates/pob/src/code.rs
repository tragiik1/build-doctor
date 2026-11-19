//! PoB build codes: URL-safe base64 of zlib-compressed build XML.

use std::io::{Read, Write};

use base64::Engine as _;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::{Error, Result};

/// Decodes a PoB build code into build XML.
///
/// Accepts padded or unpadded input and the `+`/`/` alphabet that some sites
/// still emit. Surrounding whitespace is ignored.
pub fn decode(code: &str) -> Result<String> {
    let cleaned: String = code
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect();
    let unpadded = cleaned.trim_end_matches('=');
    let compressed = URL_SAFE_NO_PAD
        .decode(unpadded)
        .map_err(|e| Error::Code(format!("not base64: {e}")))?;

    let mut xml = String::new();
    ZlibDecoder::new(compressed.as_slice())
        .read_to_string(&mut xml)
        .map_err(|e| Error::Code(format!("not zlib-compressed UTF-8: {e}")))?;
    Ok(xml)
}

/// Encodes build XML as a PoB build code.
pub fn encode(xml: &str) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(xml.as_bytes())
        .expect("writing to a Vec cannot fail");
    let compressed = encoder.finish().expect("writing to a Vec cannot fail");
    URL_SAFE.encode(compressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let xml = "<PathOfBuilding2><Build level=\"90\"/></PathOfBuilding2>";
        assert_eq!(decode(&encode(xml)).unwrap(), xml);
    }

    #[test]
    fn accepts_unpadded_and_standard_alphabet() {
        let xml = "<PathOfBuilding2>?>?>?></PathOfBuilding2>";
        let code = encode(xml);
        let variant = code
            .trim_end_matches('=')
            .replace('-', "+")
            .replace('_', "/");
        assert_eq!(decode(&format!("  {variant}\n")).unwrap(), xml);
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(decode("not a code!"), Err(Error::Code(_))));
        assert!(matches!(decode("aGVsbG8"), Err(Error::Code(_))));
    }
}
