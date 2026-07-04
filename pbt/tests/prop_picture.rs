//! picture モジュールの PBT
//!
//! 任意の有効な PICTURE ブロックのラウンドトリップを検証する。

use proptest::prelude::*;
use shiguredo_flac::picture::{Picture, PictureType};

/// メディアタイプ: 印字可能 ASCII (RFC 9639 Section 8.8)
fn media_type() -> impl Strategy<Value = String> {
    proptest::collection::vec((0x20u8..=0x7E).prop_map(|b| b as char), 0..=32)
        .prop_map(|chars| chars.into_iter().collect())
}

fn picture() -> impl Strategy<Value = Picture> {
    (
        (
            0u32..=25,
            media_type(),
            ".{0,32}",
            any::<u32>(),
            any::<u32>(),
        ),
        (
            any::<u32>(),
            any::<u32>(),
            proptest::collection::vec(any::<u8>(), 0..=256),
        ),
    )
        .prop_map(
            |(
                (type_value, media_type, description, width, height),
                (color_depth, colors, data),
            )| {
                Picture {
                    picture_type: PictureType::from_u32(type_value),
                    media_type,
                    description,
                    width,
                    height,
                    color_depth,
                    colors,
                    data,
                }
            },
        )
}

proptest! {
    #[test]
    fn roundtrip(picture in picture()) {
        let payload = picture.encode_payload().expect("エンコードに成功するはず");
        let decoded = Picture::decode(&payload).expect("デコードに成功するはず");
        prop_assert_eq!(decoded, picture);
    }

    #[test]
    fn picture_type_u32_roundtrip(value in any::<u32>()) {
        prop_assert_eq!(PictureType::from_u32(value).to_u32(), value);
    }
}
