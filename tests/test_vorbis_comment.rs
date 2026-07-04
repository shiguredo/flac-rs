//! vorbis_comment モジュールの単体テスト
//!
//! 意図的なエラーパスを検証する。ラウンドトリップ性は PBT
//! (pbt/tests/prop_vorbis_comment.rs) が担う。

use shiguredo_flac::vorbis_comment::{VorbisComment, VorbisCommentField};

#[test]
fn decode_rejects_field_without_separator() {
    // "TITLE" (= なし) のフィールド
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&5u32.to_le_bytes());
    payload.extend_from_slice(b"TITLE");
    assert!(VorbisComment::decode(&payload).is_err());
}

#[test]
fn decode_rejects_invalid_utf8_vendor() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&[0xff, 0xfe]);
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert!(VorbisComment::decode(&payload).is_err());
}

#[test]
fn decode_rejects_truncated_field_count() {
    // vendor までで切れている
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert!(VorbisComment::decode(&payload).is_err());
}

#[test]
fn decode_rejects_huge_field_length() {
    // フィールド長がペイロードを超える
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(VorbisComment::decode(&payload).is_err());
}

#[test]
fn decode_rejects_trailing_bytes() {
    let comment = VorbisComment {
        vendor: String::new(),
        fields: Vec::new(),
    };
    let mut payload = comment.encode_payload().unwrap();
    payload.push(0x00);
    assert!(VorbisComment::decode(&payload).is_err());
}

#[test]
fn encode_rejects_invalid_field_name() {
    // 空文字・= を含む・非 ASCII は拒否される (RFC 9639 Section 8.6)
    for name in ["", "NAME=", "日本語"] {
        let comment = VorbisComment {
            vendor: String::new(),
            fields: vec![VorbisCommentField {
                name: String::from(name),
                value: String::new(),
            }],
        };
        assert!(
            comment.encode_payload().is_err(),
            "{:?} は拒否されるべき",
            name
        );
    }
}

#[test]
fn encode_allows_space_in_field_name() {
    // スペースは U+0020 なので有効 (RFC 9639 Section 8.6)
    let comment = VorbisComment {
        vendor: String::new(),
        fields: vec![VorbisCommentField {
            name: String::from("FIELD NAME"),
            value: String::new(),
        }],
    };
    assert!(comment.encode_payload().is_ok());
}

#[test]
fn value_may_contain_equals_sign() {
    // 値には = を含められる (最初の = でのみ分割される)
    let comment = VorbisComment {
        vendor: String::new(),
        fields: vec![VorbisCommentField {
            name: String::from("DESCRIPTION"),
            value: String::from("a=b=c"),
        }],
    };
    let payload = comment.encode_payload().unwrap();
    assert_eq!(VorbisComment::decode(&payload).unwrap(), comment);
}
