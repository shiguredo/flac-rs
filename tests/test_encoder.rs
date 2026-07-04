//! encoder モジュールの単体テスト
//!
//! 意図的なエラーパス・境界値・決定的な信号のロスレス性を検証する。
//! 任意サンプル列のロスレス性は PBT (pbt/tests/prop_encoder.rs) が担う。

mod helpers;

use helpers::pseudo_random_samples;
use shiguredo_flac::EncodeError;
use shiguredo_flac::decoder;
use shiguredo_flac::encoder::{StreamEncoder, StreamEncoderConfig, encode};
use shiguredo_flac::metadata::{MetadataBlock, StreamInfo};
use shiguredo_flac::vorbis_comment::{VorbisComment, VorbisCommentField};

/// エンコード → デコードのロスレス性を検証する
fn assert_lossless(config: StreamEncoderConfig, samples: &[i32]) {
    let channels = config.channels;
    let encoded = encode(config, samples).expect("エンコードに成功するはず");
    let decoded = decoder::decode(&encoded).expect("デコードに成功するはず");
    assert_eq!(decoded.samples, samples, "ロスレス性が壊れている");
    assert_eq!(
        decoded.stream_info.total_samples as usize,
        samples.len() / usize::from(channels)
    );
}

#[test]
fn lossless_silence() {
    let samples = vec![0i32; 4096 * 2 + 100];
    assert_lossless(StreamEncoderConfig::default(), &samples);
}

#[test]
fn lossless_constant_signal() {
    let samples = vec![12345i32; 9000];
    let config = StreamEncoderConfig {
        channels: 1,
        ..StreamEncoderConfig::default()
    };
    assert_lossless(config, &samples);
}

#[test]
fn lossless_stereo_32bit_extremes() {
    // 32 bit の極値 (サイドチャンネルは 33 bit になる)
    let mut samples = Vec::new();
    for i in 0..1000 {
        match i % 4 {
            0 => samples.extend_from_slice(&[i32::MAX, i32::MIN]),
            1 => samples.extend_from_slice(&[i32::MIN, i32::MAX]),
            2 => samples.extend_from_slice(&[i32::MAX, i32::MAX]),
            _ => samples.extend_from_slice(&[0, -1]),
        }
    }
    let config = StreamEncoderConfig {
        bits_per_sample: 32,
        block_size: 256,
        ..StreamEncoderConfig::default()
    };
    assert_lossless(config, &samples);
}

#[test]
fn lossless_wasted_bits_signal() {
    // 下位 4 bit が常に 0 の信号 (wasted bits が効く)
    let samples: Vec<i32> = (0..8000).map(|i| ((i % 1000) - 500) * 16).collect();
    let config = StreamEncoderConfig {
        channels: 1,
        ..StreamEncoderConfig::default()
    };
    assert_lossless(config, &samples);
}

#[test]
fn lossless_empty_stream() {
    // サンプル 0 個 (メタデータのみのストリーム)
    assert_lossless(StreamEncoderConfig::default(), &[]);
}

#[test]
fn lossless_side_channel_of_zeros_and_minus_one() {
    // fuzzing が発見した回帰ケース: mid-side のサイドチャンネルが
    // wasted bits 適用後に [0, 0, 0, 0, -1] となり、-1 の magnitude
    // (!(-1) = 0) を「全サンプルが 0」と誤判定すると -1 が消える
    let samples: Vec<i32> = vec![
        -33554432, -33554432, 33554431, 33554431, 33554431, 33554431, -33554432, -33554432, -1,
        4095,
    ];
    let config = StreamEncoderConfig {
        channels: 2,
        bits_per_sample: 26,
        max_lpc_order: 28,
        block_size: 64,
        ..StreamEncoderConfig::default()
    };
    assert_lossless(config, &samples);
}

#[test]
fn lossless_uncommon_bit_depth_and_sample_rate() {
    // 13 bit / 12345 Hz はフレームヘッダーで直接表現できない
    let samples = pseudo_random_samples(5000, 13, 11);
    let config = StreamEncoderConfig {
        channels: 1,
        bits_per_sample: 13,
        sample_rate: 12345,
        ..StreamEncoderConfig::default()
    };
    assert_lossless(config, &samples);
}

#[test]
fn lossless_minimum_bit_depth() {
    let samples = pseudo_random_samples(1000, 4, 17);
    let config = StreamEncoderConfig {
        channels: 1,
        bits_per_sample: 4,
        ..StreamEncoderConfig::default()
    };
    assert_lossless(config, &samples);
}

#[test]
fn lossless_8_channels() {
    let samples = pseudo_random_samples(1000 * 8, 16, 5);
    let config = StreamEncoderConfig {
        channels: 8,
        ..StreamEncoderConfig::default()
    };
    assert_lossless(config, &samples);
}

#[test]
fn compression_beats_verbatim_for_predictable_signal() {
    // 予測可能な信号では非圧縮 (サンプルサイズ x サンプル数) より小さくなる
    let samples: Vec<i32> = (0..44100)
        .map(|i| {
            let phase = i % 441;
            (phase - 220) * 50
        })
        .collect();
    let config = StreamEncoderConfig {
        channels: 1,
        ..StreamEncoderConfig::default()
    };
    let encoded = encode(config, &samples).unwrap();
    assert!(
        encoded.len() < samples.len() * 2,
        "圧縮後 {} バイトは元の {} バイトより小さいはず",
        encoded.len(),
        samples.len() * 2
    );
}

#[test]
fn metadata_blocks_are_preserved() {
    let comment = VorbisComment {
        vendor: String::from("shiguredo_flac"),
        fields: vec![VorbisCommentField {
            name: String::from("TITLE"),
            value: String::from("テスト"),
        }],
    };
    let config = StreamEncoderConfig {
        metadata: vec![
            MetadataBlock::VorbisComment(comment.clone()),
            MetadataBlock::Padding { size: 64 },
        ],
        ..StreamEncoderConfig::default()
    };
    let encoded = encode(config, &[1, 2, 3, 4]).unwrap();
    let decoded = decoder::decode(&encoded).unwrap();
    // STREAMINFO + VORBIS_COMMENT + PADDING
    assert_eq!(decoded.metadata.len(), 3);
    assert_eq!(decoded.metadata[1], MetadataBlock::VorbisComment(comment));
    assert_eq!(decoded.metadata[2], MetadataBlock::Padding { size: 64 });
}

#[test]
fn rejects_out_of_range_sample() {
    let config = StreamEncoderConfig {
        channels: 1,
        bits_per_sample: 8,
        ..StreamEncoderConfig::default()
    };
    let mut encoder = StreamEncoder::new(config).unwrap();
    assert!(matches!(
        encoder.push_samples(&[128]),
        Err(EncodeError::SampleOutOfRange { .. })
    ));
    let config = StreamEncoderConfig {
        channels: 1,
        bits_per_sample: 8,
        ..StreamEncoderConfig::default()
    };
    let mut encoder = StreamEncoder::new(config).unwrap();
    assert!(encoder.push_samples(&[127, -128]).is_ok());
}

#[test]
fn rejects_unaligned_sample_count() {
    let mut encoder = StreamEncoder::new(StreamEncoderConfig::default()).unwrap();
    assert!(matches!(
        encoder.push_samples(&[1, 2, 3]),
        Err(EncodeError::UnalignedSamples { .. })
    ));
}

#[test]
fn rejects_invalid_config() {
    for config in [
        StreamEncoderConfig {
            sample_rate: 0,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            sample_rate: 0x10_0000,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            channels: 0,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            channels: 9,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            bits_per_sample: 3,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            bits_per_sample: 33,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            block_size: 15,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            max_lpc_order: 33,
            ..StreamEncoderConfig::default()
        },
        StreamEncoderConfig {
            max_partition_order: 16,
            ..StreamEncoderConfig::default()
        },
    ] {
        assert!(StreamEncoder::new(config).is_err());
    }
}

#[test]
fn rejects_supplied_streaminfo_metadata() {
    let config = StreamEncoderConfig {
        metadata: vec![MetadataBlock::StreamInfo(StreamInfo {
            min_block_size: 16,
            max_block_size: 16,
            min_frame_size: 0,
            max_frame_size: 0,
            sample_rate: 44100,
            channels: 2,
            bits_per_sample: 16,
            total_samples: 0,
            md5: [0; 16],
        })],
        ..StreamEncoderConfig::default()
    };
    assert!(StreamEncoder::new(config).is_err());
}

#[test]
fn rejects_duplicate_vorbis_comment_metadata() {
    let comment = MetadataBlock::VorbisComment(VorbisComment {
        vendor: String::new(),
        fields: Vec::new(),
    });
    let config = StreamEncoderConfig {
        metadata: vec![comment.clone(), comment],
        ..StreamEncoderConfig::default()
    };
    assert!(StreamEncoder::new(config).is_err());
}

/// エンコード結果の STREAMINFO にフレームサイズの実測値が入る
#[test]
fn streaminfo_frame_sizes_are_recorded() {
    let samples = pseudo_random_samples(4096 * 3 * 2, 16, 99);
    let encoded = encode(StreamEncoderConfig::default(), &samples).unwrap();
    let decoded = decoder::decode(&encoded).unwrap();
    let info = &decoded.stream_info;
    assert!(info.min_frame_size > 0);
    assert!(info.min_frame_size <= info.max_frame_size);
    assert_eq!(info.min_block_size, 4096);
    assert_eq!(info.max_block_size, 4096);
}
