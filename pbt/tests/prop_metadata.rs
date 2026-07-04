//! metadata モジュールの PBT
//!
//! 各メタデータブロックのエンコード → デコードのラウンドトリップを検証する。

use proptest::prelude::*;
use shiguredo_flac::metadata::{Application, MetadataBlock, SeekPoint, SeekTable, StreamInfo};

/// 有効な STREAMINFO の strategy
fn stream_info() -> impl Strategy<Value = StreamInfo> {
    (
        (16u16..=65535, 0u16..=65535),
        0u32..=0xFF_FFFF,
        0u32..=0xFF_FFFF,
        0u32..=0xF_FFFF,
        1u8..=8,
        4u8..=32,
        0u64..=0xF_FFFF_FFFF,
        proptest::array::uniform16(any::<u8>()),
    )
        .prop_map(
            |(
                (min_block, extra),
                min_frame_size,
                max_frame_size,
                sample_rate,
                channels,
                bits_per_sample,
                total_samples,
                md5,
            )| {
                // max_block_size >= min_block_size を保証する
                let max_block = min_block.saturating_add(extra).max(min_block);
                StreamInfo {
                    min_block_size: min_block,
                    max_block_size: max_block,
                    min_frame_size,
                    max_frame_size,
                    sample_rate,
                    channels,
                    bits_per_sample,
                    total_samples,
                    md5,
                }
            },
        )
}

/// 有効なシークテーブルの strategy (昇順・一意、プレースホルダーは末尾)
fn seek_table() -> impl Strategy<Value = SeekTable> {
    (
        proptest::collection::btree_set(0u64..u64::MAX, 0..=32),
        proptest::collection::vec((any::<u64>(), any::<u16>()), 0..=32),
        0usize..=3,
    )
        .prop_map(|(sample_numbers, details, placeholders)| {
            let mut points: Vec<SeekPoint> = sample_numbers
                .into_iter()
                .zip(details)
                .map(
                    |(sample_number, (stream_offset, frame_samples))| SeekPoint {
                        sample_number,
                        stream_offset,
                        frame_samples,
                    },
                )
                .collect();
            for _ in 0..placeholders {
                points.push(SeekPoint {
                    sample_number: SeekPoint::PLACEHOLDER,
                    stream_offset: 0,
                    frame_samples: 0,
                });
            }
            SeekTable { points }
        })
}

/// APPLICATION ブロックの strategy
fn application() -> impl Strategy<Value = Application> {
    (
        proptest::array::uniform4(any::<u8>()),
        proptest::collection::vec(any::<u8>(), 0..=256),
    )
        .prop_map(|(id, data)| Application { id, data })
}

proptest! {
    #[test]
    fn stream_info_roundtrip(info in stream_info()) {
        let payload = info.encode_payload().expect("エンコードに成功するはず");
        prop_assert_eq!(payload.len(), 34);
        let decoded = StreamInfo::decode(&payload).expect("デコードに成功するはず");
        prop_assert_eq!(decoded, info);
    }

    #[test]
    fn seek_table_roundtrip(table in seek_table()) {
        let payload = table.encode_payload().expect("エンコードに成功するはず");
        prop_assert_eq!(payload.len(), table.points.len() * 18);
        let decoded = SeekTable::decode(&payload).expect("デコードに成功するはず");
        prop_assert_eq!(decoded, table);
    }

    #[test]
    fn application_roundtrip(app in application()) {
        let payload = app.encode_payload().expect("エンコードに成功するはず");
        let decoded = Application::decode(&payload).expect("デコードに成功するはず");
        prop_assert_eq!(decoded, app);
    }

    /// ヘッダー込みのブロックエンコードで last フラグ・タイプ・サイズが正しい
    #[test]
    fn block_header_roundtrip(size in 0u32..=1024, is_last in any::<bool>()) {
        let block = MetadataBlock::Padding { size };
        let encoded = block.encode(is_last).expect("エンコードに成功するはず");
        prop_assert_eq!(encoded.len(), 4 + size as usize);
        prop_assert_eq!(encoded[0] & 0x80 != 0, is_last);
        prop_assert_eq!(encoded[0] & 0x7F, 1);
        let decoded_size = usize::from(encoded[1]) << 16
            | usize::from(encoded[2]) << 8
            | usize::from(encoded[3]);
        prop_assert_eq!(decoded_size, size as usize);
        let decoded = MetadataBlock::decode(1, &encoded[4..]).expect("デコードに成功するはず");
        prop_assert_eq!(decoded, block);
    }
}
