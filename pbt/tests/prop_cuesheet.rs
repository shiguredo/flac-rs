//! cuesheet モジュールの PBT
//!
//! 任意の有効なキューシートのラウンドトリップを検証する。

use proptest::prelude::*;
use shiguredo_flac::cuesheet::{Cuesheet, CuesheetTrack, CuesheetTrackIndex};

/// メディアカタログ番号: 印字可能 ASCII の右 0x00 パディング
/// (RFC 9639 Section 8.7)
fn media_catalog_number() -> impl Strategy<Value = [u8; 128]> {
    proptest::collection::vec(0x20u8..=0x7E, 0..=128).prop_map(|text| {
        let mut mcn = [0u8; 128];
        mcn[..text.len()].copy_from_slice(&text);
        mcn
    })
}

/// トラック 1 個分の中身 (番号以外) の strategy
///
/// インデックスポイント番号は 0 または 1 から始まる連番でなければならない
/// (RFC 9639 Section 8.7.1.1)。リードアウト以外は 1 個以上必要なので
/// 1..=4 個生成する。
fn track_body() -> impl Strategy<Value = CuesheetTrack> {
    (
        any::<u64>(),
        proptest::array::uniform12(any::<u8>()),
        any::<bool>(),
        any::<bool>(),
        0u8..=1,
        proptest::collection::vec(any::<u64>(), 1..=4),
    )
        .prop_map(
            |(offset_samples, isrc, is_non_audio, pre_emphasis, first_number, index_offsets)| {
                let index_points = index_offsets
                    .into_iter()
                    .enumerate()
                    .map(|(i, offset)| CuesheetTrackIndex {
                        offset_samples: offset,
                        number: first_number + i as u8,
                    })
                    .collect();
                CuesheetTrack {
                    offset_samples,
                    number: 0, // 呼び出し側で一意な番号を割り当てる
                    isrc,
                    is_non_audio,
                    pre_emphasis,
                    index_points,
                }
            },
        )
}

/// 有効なキューシートの strategy
///
/// トラック番号は一意 (RFC 9639 Section 8.7.1)、最終トラックはリードアウトで
/// インデックスポイントを持たない (RFC 9639 Section 8.7.1)。
fn cuesheet() -> impl Strategy<Value = Cuesheet> {
    (
        media_catalog_number(),
        any::<u64>(),
        any::<bool>(),
        proptest::collection::btree_set(1u8..=255, 1..=8),
        proptest::collection::vec(track_body(), 8),
        (any::<u64>(), proptest::array::uniform12(any::<u8>())),
    )
        .prop_map(
            |(
                media_catalog_number,
                lead_in_samples,
                is_cdda,
                track_numbers,
                bodies,
                (lead_out_offset, lead_out_isrc),
            )| {
                // 一意な番号の集合からトラックを作り、最後をリードアウトにする
                let count = track_numbers.len();
                let mut tracks: Vec<CuesheetTrack> = track_numbers
                    .into_iter()
                    .zip(bodies)
                    .map(|(number, mut body)| {
                        body.number = number;
                        body
                    })
                    .collect();
                tracks[count - 1] = CuesheetTrack {
                    offset_samples: lead_out_offset,
                    number: tracks[count - 1].number,
                    isrc: lead_out_isrc,
                    is_non_audio: false,
                    pre_emphasis: false,
                    index_points: Vec::new(),
                };
                Cuesheet {
                    media_catalog_number,
                    lead_in_samples,
                    is_cdda,
                    tracks,
                }
            },
        )
}

proptest! {
    #[test]
    fn roundtrip(cuesheet in cuesheet()) {
        let payload = cuesheet.encode_payload().expect("エンコードに成功するはず");
        let decoded = Cuesheet::decode(&payload).expect("デコードに成功するはず");
        prop_assert_eq!(decoded, cuesheet);
    }
}
