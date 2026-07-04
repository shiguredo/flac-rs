//! vorbis_comment モジュールの PBT
//!
//! 任意の有効なフィールド名・値でのラウンドトリップを検証する。

use proptest::prelude::*;
use shiguredo_flac::vorbis_comment::{VorbisComment, VorbisCommentField};

/// 有効なフィールド名: U+0020 から U+007E (U+003D の = を除く)
/// (RFC 9639 Section 8.6)
fn field_name() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![0x20u8..=0x3C, 0x3Eu8..=0x7E].prop_map(|b| b as char),
        1..=16,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

/// フィールド値: 任意の UTF-8 文字列 (= を含んでもよい)
fn field_value() -> impl Strategy<Value = String> {
    ".{0,32}"
}

fn vorbis_comment() -> impl Strategy<Value = VorbisComment> {
    (
        ".{0,32}",
        proptest::collection::vec(
            (field_name(), field_value())
                .prop_map(|(name, value)| VorbisCommentField { name, value }),
            0..=8,
        ),
    )
        .prop_map(|(vendor, fields)| VorbisComment { vendor, fields })
}

proptest! {
    #[test]
    fn roundtrip(comment in vorbis_comment()) {
        let payload = comment.encode_payload().expect("エンコードに成功するはず");
        let decoded = VorbisComment::decode(&payload).expect("デコードに成功するはず");
        prop_assert_eq!(decoded, comment);
    }
}
