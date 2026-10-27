//! Cover art for Ogg files (Vorbis and Opus).
//!
//! FFmpeg's Ogg muxers cannot store an attached picture, but the Ogg format
//! has a standard place for it: a `METADATA_BLOCK_PICTURE` comment holding a
//! base64-encoded FLAC picture block. Konvert builds that comment itself and
//! hands it to FFmpeg through an `ffmetadata` file (a cover is far too long
//! for a command line). Pure functions only.

/// Picture type "front cover" in the FLAC picture block.
const FRONT_COVER: u32 = 3;

/// MIME type of a cover picture from its FFmpeg codec name.
pub fn cover_mime(codec: &str) -> Option<&'static str> {
    match codec {
        "mjpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        _ => None,
    }
}

/// FLAC `METADATA_BLOCK_PICTURE` body (without the metadata block header).
/// Unknown dimensions are written as 0, which readers accept.
pub fn picture_block(mime: &str, width: u32, height: u32, data: &[u8]) -> Vec<u8> {
    let mut block = Vec::with_capacity(32 + mime.len() + data.len());
    for value in [FRONT_COVER, mime.len() as u32] {
        block.extend_from_slice(&value.to_be_bytes());
    }
    block.extend_from_slice(mime.as_bytes());
    // Empty description, size, color depth (24 bits), no palette, data length.
    for value in [0, width, height, 24, 0, data.len() as u32] {
        block.extend_from_slice(&value.to_be_bytes());
    }
    block.extend_from_slice(data);
    block
}

/// `ffmetadata` file whose global section holds the picture comment.
pub fn ffmetadata_with_picture(block: &[u8]) -> String {
    format!(
        ";FFMETADATA1\nMETADATA_BLOCK_PICTURE={}\n",
        escape_ffmetadata(&base64(block))
    )
}

/// Escapes the characters `ffmetadata` treats specially (`=`, `;`, `#`, `\`, newline).
fn escape_ffmetadata(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for c in value.chars() {
        if matches!(c, '=' | ';' | '#' | '\\' | '\n') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Standard base64 with padding (RFC 4648).
pub(crate) fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in cases {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain}");
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn picture_block_layout() {
        let block = picture_block("image/png", 640, 480, b"DATA");
        let u32_at = |i: usize| u32::from_be_bytes(block[i..i + 4].try_into().unwrap());
        assert_eq!(u32_at(0), 3);
        assert_eq!(u32_at(4), 9);
        assert_eq!(&block[8..17], b"image/png");
        // Description length, width, height, depth, colors, data length.
        assert_eq!(
            [17, 21, 25, 29, 33, 37].map(u32_at),
            [0, 640, 480, 24, 0, 4]
        );
        assert_eq!(&block[41..], b"DATA");
    }

    #[test]
    fn ffmetadata_escapes_the_padding() {
        let text = ffmetadata_with_picture(b"f");
        assert_eq!(text, ";FFMETADATA1\nMETADATA_BLOCK_PICTURE=Zg\\=\\=\n");
        assert_eq!(escape_ffmetadata("a=b;c#d\\e"), "a\\=b\\;c\\#d\\\\e");
        assert_eq!(cover_mime("mjpeg"), Some("image/jpeg"));
        assert_eq!(cover_mime("bmp"), None);
    }
}
