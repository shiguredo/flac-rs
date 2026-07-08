//! cuesheet モジュールの単体テスト
//!
//! 意図的なエラーパスを検証する。ラウンドトリップ性は PBT
//! (pbt/tests/prop_cuesheet.rs) が担う。

use shiguredo_flac::cuesheet::{Cuesheet, CuesheetTrack, CuesheetTrackIndex};

fn catalog_number(text: &str) -> [u8; 128] {
    let mut mcn = [0u8; 128];
    mcn[..text.len()].copy_from_slice(text.as_bytes());
    mcn
}

fn sample_cuesheet() -> Cuesheet {
    Cuesheet {
        media_catalog_number: catalog_number("1234567890123"),
        lead_in_samples: 88200,
        is_cdda: true,
        tracks: vec![
            CuesheetTrack {
                offset_samples: 0,
                number: 1,
                isrc: *b"JPAB01234567",
                is_non_audio: false,
                pre_emphasis: false,
                index_points: vec![
                    CuesheetTrackIndex {
                        offset_samples: 0,
                        number: 1,
                    },
                    CuesheetTrackIndex {
                        offset_samples: 588,
                        number: 2,
                    },
                ],
            },
            // リードアウトトラック (インデックスポイントなし)
            CuesheetTrack {
                offset_samples: 44100 * 60,
                number: 170,
                isrc: [0u8; 12],
                is_non_audio: false,
                pre_emphasis: false,
                index_points: Vec::new(),
            },
        ],
    }
}

#[test]
fn media_catalog_number_str_strips_padding() {
    let cuesheet = sample_cuesheet();
    assert_eq!(cuesheet.media_catalog_number_str(), "1234567890123");
}

#[test]
fn decode_rejects_truncated_payload() {
    let payload = sample_cuesheet()
        .encode_payload()
        .expect("CUESHEET エンコードに成功するはず");
    assert!(Cuesheet::decode(&payload[..payload.len() - 1]).is_err());
    assert!(Cuesheet::decode(&payload[..100]).is_err());
}

#[test]
fn decode_rejects_trailing_bytes() {
    let mut payload = sample_cuesheet()
        .encode_payload()
        .expect("CUESHEET エンコードに成功するはず");
    payload.push(0);
    assert!(Cuesheet::decode(&payload).is_err());
}

#[test]
fn rejects_zero_tracks() {
    // エンコード側: リードアウトトラック必須 (RFC 9639 Section 8.7)
    let mut cuesheet = sample_cuesheet();
    cuesheet.tracks.clear();
    assert!(cuesheet.encode_payload().is_err());

    // デコード側: トラック数 0 のペイロードを拒否する
    let mut payload = sample_cuesheet()
        .encode_payload()
        .expect("CUESHEET エンコードに成功するはず");
    payload[128 + 8 + 1 + 258] = 0; // トラック数フィールドを 0 にする
    let payload = &payload[..128 + 8 + 1 + 258 + 1];
    assert!(Cuesheet::decode(payload).is_err());
}

#[test]
fn encode_rejects_track_number_zero() {
    // トラック番号 0 は CD-DA のリードイン用に予約されている
    // (RFC 9639 Section 8.7.1)
    let mut cuesheet = sample_cuesheet();
    cuesheet.tracks[0].number = 0;
    assert!(cuesheet.encode_payload().is_err());
}

#[test]
fn decode_rejects_non_ascii_catalog_number() {
    let mut payload = sample_cuesheet()
        .encode_payload()
        .expect("CUESHEET エンコードに成功するはず");
    payload[0] = 0xFF;
    assert!(Cuesheet::decode(&payload).is_err());
}

#[test]
fn encode_rejects_non_ascii_catalog_number() {
    let mut cuesheet = sample_cuesheet();
    cuesheet.media_catalog_number[0] = 0xFF;
    assert!(cuesheet.encode_payload().is_err());
}

#[test]
fn encode_rejects_catalog_number_with_embedded_nul() {
    // 0x00 の後に非 0x00 が続くのは不正 (右パディングのみ許される)
    let mut cuesheet = sample_cuesheet();
    cuesheet.media_catalog_number = [0u8; 128];
    cuesheet.media_catalog_number[1] = b'A';
    assert!(cuesheet.encode_payload().is_err());
}
