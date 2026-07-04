//! picture モジュールの単体テスト
//!
//! 意図的なエラーパスを検証する。ラウンドトリップ性は PBT
//! (pbt/tests/prop_picture.rs) が担う。

use shiguredo_flac::picture::{Picture, PictureType};

fn sample_picture() -> Picture {
    Picture {
        picture_type: PictureType::FrontCover,
        media_type: String::from("image/png"),
        description: String::from("カバーアート"),
        width: 640,
        height: 480,
        color_depth: 24,
        colors: 0,
        data: vec![0x89, 0x50, 0x4E, 0x47],
    }
}

#[test]
fn uri_picture_is_detected() {
    let picture = Picture {
        media_type: String::from(Picture::URI_MEDIA_TYPE),
        data: b"https://example.com/cover.png".to_vec(),
        ..sample_picture()
    };
    assert!(picture.is_uri());
    assert!(!sample_picture().is_uri());
}

#[test]
fn picture_type_conversion_roundtrip() {
    // 定義済み 0-20 と予約済みの値の両方
    for value in 0..=25 {
        assert_eq!(PictureType::from_u32(value).to_u32(), value);
    }
    assert_eq!(PictureType::from_u32(21), PictureType::Reserved(21));
}

#[test]
fn decode_rejects_truncated_payload() {
    let payload = sample_picture().encode_payload().unwrap();
    for len in [0, 3, 7, payload.len() - 1] {
        assert!(Picture::decode(&payload[..len]).is_err(), "長さ {}", len);
    }
}

#[test]
fn decode_rejects_trailing_bytes() {
    let mut payload = sample_picture().encode_payload().unwrap();
    payload.push(0);
    assert!(Picture::decode(&payload).is_err());
}

#[test]
fn decode_rejects_non_ascii_media_type() {
    let picture = sample_picture();
    let mut payload = picture.encode_payload().unwrap();
    // media_type の先頭バイトを非 ASCII にする
    payload[8] = 0xFF;
    assert!(Picture::decode(&payload).is_err());
}

#[test]
fn encode_rejects_non_ascii_media_type() {
    let picture = Picture {
        media_type: String::from("画像/png"),
        ..sample_picture()
    };
    assert!(picture.encode_payload().is_err());
}
