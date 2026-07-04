//! metadata モジュールの単体テスト
//!
//! RFC 9639 の実例と意図的なエラーパスを検証する。
//! ラウンドトリップ性は PBT (pbt/tests/prop_metadata.rs) が担う。

use shiguredo_flac::metadata::{MetadataBlock, SeekPoint, SeekTable, StreamInfo};

/// RFC 9639 Appendix D.1 の STREAMINFO ペイロード (0x08-0x29 の 34 バイト)
fn appendix_d1_streaminfo_payload() -> Vec<u8> {
    vec![
        0x10, 0x00, // min block size 4096
        0x10, 0x00, // max block size 4096
        0x00, 0x00, 0x0f, // min frame size 15
        0x00, 0x00, 0x0f, // max frame size 15
        0x0a, 0xc4, 0x42, // sample rate 44100 + channels + bit depth (一部)
        0xf0, // bit depth 続き + total samples 先頭
        0x00, 0x00, 0x00, 0x01, // total samples 1
        0x3e, 0x84, 0xb4, 0x18, 0x07, 0xdc, 0x69, 0x03, // MD5
        0x07, 0x58, 0x6a, 0x3d, 0xad, 0x1a, 0x2e, 0x0f, // MD5 続き
    ]
}

#[test]
fn decode_streaminfo_rfc9639_appendix_d1() {
    let info = StreamInfo::decode(&appendix_d1_streaminfo_payload()).unwrap();
    assert_eq!(info.min_block_size, 4096);
    assert_eq!(info.max_block_size, 4096);
    assert_eq!(info.min_frame_size, 15);
    assert_eq!(info.max_frame_size, 15);
    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.channels, 2);
    assert_eq!(info.bits_per_sample, 16);
    assert_eq!(info.total_samples, 1);
    assert_eq!(
        info.md5,
        [
            0x3e, 0x84, 0xb4, 0x18, 0x07, 0xdc, 0x69, 0x03, 0x07, 0x58, 0x6a, 0x3d, 0xad, 0x1a,
            0x2e, 0x0f
        ]
    );
    // エンコードすると元のバイト列に戻る
    assert_eq!(
        info.encode_payload().unwrap(),
        appendix_d1_streaminfo_payload()
    );
}

#[test]
fn streaminfo_rejects_wrong_size() {
    assert!(StreamInfo::decode(&[0u8; 33]).is_err());
    assert!(StreamInfo::decode(&[0u8; 35]).is_err());
}

#[test]
fn streaminfo_rejects_small_block_size() {
    // ブロックサイズ 15 は forbidden (RFC 9639 Section 5 Table 1)
    let mut payload = appendix_d1_streaminfo_payload();
    payload[0] = 0x00;
    payload[1] = 0x0f;
    assert!(StreamInfo::decode(&payload).is_err());
}

#[test]
fn streaminfo_rejects_min_block_size_exceeding_max() {
    let mut payload = appendix_d1_streaminfo_payload();
    // min 4096 のまま max を 16 にする
    payload[2] = 0x00;
    payload[3] = 0x10;
    assert!(StreamInfo::decode(&payload).is_err());
}

#[test]
fn seek_table_rejects_unsorted_points() {
    let table = SeekTable {
        points: vec![
            SeekPoint {
                sample_number: 44100,
                stream_offset: 0,
                frame_samples: 4096,
            },
            SeekPoint {
                sample_number: 0,
                stream_offset: 0,
                frame_samples: 4096,
            },
        ],
    };
    assert!(table.encode_payload().is_err());
}

#[test]
fn seek_table_rejects_duplicate_sample_numbers() {
    let point = SeekPoint {
        sample_number: 42,
        stream_offset: 0,
        frame_samples: 4096,
    };
    let table = SeekTable {
        points: vec![point, point],
    };
    assert!(table.encode_payload().is_err());
}

#[test]
fn seek_table_rejects_placeholder_before_real_point() {
    let table = SeekTable {
        points: vec![
            SeekPoint {
                sample_number: SeekPoint::PLACEHOLDER,
                stream_offset: 0,
                frame_samples: 0,
            },
            SeekPoint {
                sample_number: 0,
                stream_offset: 0,
                frame_samples: 4096,
            },
        ],
    };
    assert!(table.encode_payload().is_err());
}

#[test]
fn seek_table_allows_multiple_placeholders_at_end() {
    let placeholder = SeekPoint {
        sample_number: SeekPoint::PLACEHOLDER,
        stream_offset: 0,
        frame_samples: 0,
    };
    assert!(placeholder.is_placeholder());
    let table = SeekTable {
        points: vec![
            SeekPoint {
                sample_number: 0,
                stream_offset: 0,
                frame_samples: 4096,
            },
            placeholder,
            placeholder,
        ],
    };
    assert!(table.encode_payload().is_ok());
}

#[test]
fn seek_table_rejects_truncated_payload() {
    assert!(SeekTable::decode(&[0u8; 17]).is_err());
}

#[test]
fn application_rejects_short_payload() {
    use shiguredo_flac::metadata::Application;
    assert!(Application::decode(&[0u8; 3]).is_err());
}

#[test]
fn padding_block_header_encoding() {
    let block = MetadataBlock::Padding { size: 10 };
    let encoded = block.encode(true).unwrap();
    // ヘッダー: last フラグ + タイプ 1、サイズ 10
    assert_eq!(encoded[0], 0x81);
    assert_eq!(&encoded[1..4], &[0x00, 0x00, 0x0a]);
    assert_eq!(encoded.len(), 4 + 10);
    let encoded = block.encode(false).unwrap();
    assert_eq!(encoded[0], 0x01);
}

#[test]
fn unknown_block_preserves_data() {
    let block = MetadataBlock::decode(100, &[0xde, 0xad]).unwrap();
    assert_eq!(
        block,
        MetadataBlock::Unknown {
            block_type: 100,
            data: vec![0xde, 0xad],
        }
    );
    assert_eq!(block.encode_payload().unwrap(), vec![0xde, 0xad]);
}

#[test]
fn forbidden_block_type_127() {
    assert!(MetadataBlock::decode(127, &[]).is_err());
}
