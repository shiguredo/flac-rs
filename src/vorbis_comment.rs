//! VORBIS_COMMENT メタデータブロック (RFC 9639 Section 8.6)
//!
//! 人間可読なメタデータ (いわゆる FLAC タグ) を UTF-8 で保持する。
//! FLAC の他の部分と異なり、長さフィールドは little-endian で格納される
//! (RFC 9639 Section 5)。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{DecodeError, EncodeError};
use crate::metadata::MAX_METADATA_PAYLOAD_SIZE;

/// VORBIS_COMMENT のフィールド (name=value)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VorbisCommentField {
    /// フィールド名。U+0020 から U+007E (U+003D の = を除く) のみ使用できる
    pub name: String,
    /// フィールド値 (任意の UTF-8 文字列)
    pub value: String,
}

/// VORBIS_COMMENT メタデータブロック (RFC 9639 Section 8.6)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VorbisComment {
    /// ファイルを生成したプログラム名
    pub vendor: String,
    /// フィールド列
    pub fields: Vec<VorbisCommentField>,
}

/// フィールド名が有効か検証する
///
/// RFC 9639 Section 8.6: フィールド名は U+0020 から U+007E (U+003D を除く)。
fn is_valid_field_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| (0x20..=0x7E).contains(&b) && b != b'=')
}

/// ペイロードから little-endian の 32 bit 長を読み、続くバイト列を取り出す
fn read_length_prefixed<'a>(
    payload: &'a [u8],
    pos: &mut usize,
    what: &str,
) -> Result<&'a [u8], DecodeError> {
    let len_end = pos.checked_add(4).ok_or_else(|| {
        DecodeError::InvalidData(format!("{} length overflows (RFC 9639 Section 8.6)", what))
    })?;
    if len_end > payload.len() {
        return Err(DecodeError::InvalidData(format!(
            "vorbis comment is truncated while reading {} length (RFC 9639 Section 8.6)",
            what
        )));
    }
    let len = u32::from_le_bytes(
        payload[*pos..len_end]
            .try_into()
            .expect("4 バイト固定 (実装バグ)"),
    ) as usize;
    let data_end = len_end.checked_add(len).ok_or_else(|| {
        DecodeError::InvalidData(format!("{} length overflows (RFC 9639 Section 8.6)", what))
    })?;
    if data_end > payload.len() {
        return Err(DecodeError::InvalidData(format!(
            "vorbis comment is truncated while reading {} of {} bytes (RFC 9639 Section 8.6)",
            what, len
        )));
    }
    let data = &payload[len_end..data_end];
    *pos = data_end;
    Ok(data)
}

impl VorbisComment {
    /// VORBIS_COMMENT ペイロードをデコードする
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut pos = 0;

        // vendor 文字列 (長さ + UTF-8)
        let vendor_bytes = read_length_prefixed(payload, &mut pos, "vendor string")?;
        let vendor = String::from_utf8(vendor_bytes.to_vec()).map_err(|_| {
            DecodeError::InvalidData(String::from(
                "vendor string is not valid UTF-8 (RFC 9639 Section 8.6)",
            ))
        })?;

        // フィールド数 (little-endian 32 bit)
        if pos + 4 > payload.len() {
            return Err(DecodeError::InvalidData(String::from(
                "vorbis comment is truncated while reading field count (RFC 9639 Section 8.6)",
            )));
        }
        let field_count = u32::from_le_bytes(
            payload[pos..pos + 4]
                .try_into()
                .expect("4 バイト固定 (実装バグ)"),
        );
        pos += 4;

        let mut fields = Vec::new();
        for _ in 0..field_count {
            let field_bytes = read_length_prefixed(payload, &mut pos, "field")?;
            let field = core::str::from_utf8(field_bytes).map_err(|_| {
                DecodeError::InvalidData(String::from(
                    "vorbis comment field is not valid UTF-8 (RFC 9639 Section 8.6)",
                ))
            })?;
            // name=value に分割する。= がないフィールドは不正
            let Some((name, value)) = field.split_once('=') else {
                return Err(DecodeError::InvalidData(String::from(
                    "vorbis comment field has no = separator (RFC 9639 Section 8.6)",
                )));
            };
            if !is_valid_field_name(name) {
                return Err(DecodeError::InvalidData(format!(
                    "vorbis comment field name {:?} contains invalid characters (RFC 9639 Section 8.6)",
                    name
                )));
            }
            fields.push(VorbisCommentField {
                name: String::from(name),
                value: String::from(value),
            });
        }

        // 末尾に余りがあっても致命的ではないが、フォーマット違反として扱う
        if pos != payload.len() {
            return Err(DecodeError::InvalidData(format!(
                "vorbis comment has {} trailing bytes (RFC 9639 Section 8.6)",
                payload.len() - pos
            )));
        }

        Ok(Self { vendor, fields })
    }

    /// VORBIS_COMMENT ペイロードにエンコードする
    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        let vendor_len = u32::try_from(self.vendor.len()).map_err(|_| {
            EncodeError::InvalidMetadata(String::from(
                "vendor string exceeds 32-bit length (RFC 9639 Section 8.6)",
            ))
        })?;
        out.extend_from_slice(&vendor_len.to_le_bytes());
        out.extend_from_slice(self.vendor.as_bytes());

        let field_count = u32::try_from(self.fields.len()).map_err(|_| {
            EncodeError::InvalidMetadata(String::from(
                "too many vorbis comment fields (RFC 9639 Section 8.6)",
            ))
        })?;
        out.extend_from_slice(&field_count.to_le_bytes());

        for field in &self.fields {
            if !is_valid_field_name(&field.name) {
                return Err(EncodeError::InvalidMetadata(format!(
                    "vorbis comment field name {:?} contains invalid characters (RFC 9639 Section 8.6)",
                    field.name
                )));
            }
            let field_len = field
                .name
                .len()
                .checked_add(1)
                .and_then(|n| n.checked_add(field.value.len()))
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| {
                    EncodeError::InvalidMetadata(String::from(
                        "vorbis comment field exceeds 32-bit length (RFC 9639 Section 8.6)",
                    ))
                })?;
            out.extend_from_slice(&field_len.to_le_bytes());
            out.extend_from_slice(field.name.as_bytes());
            out.push(b'=');
            out.extend_from_slice(field.value.as_bytes());
        }

        if out.len() > MAX_METADATA_PAYLOAD_SIZE {
            return Err(EncodeError::InvalidMetadata(format!(
                "vorbis comment payload {} bytes exceeds 24-bit size limit (RFC 9639 Section 8.1)",
                out.len()
            )));
        }
        Ok(out)
    }
}
