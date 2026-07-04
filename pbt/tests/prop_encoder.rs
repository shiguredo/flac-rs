//! encoder モジュールの PBT
//!
//! FLAC の根幹であるロスレス性 (エンコード → デコードで元のサンプル列と
//! 完全一致) を任意の入力に対して検証する。

use pbt::{AudioInput, audio_input, stereo_input};
use proptest::prelude::*;
use shiguredo_flac::decoder;
use shiguredo_flac::encoder::{StreamEncoder, StreamEncoderConfig, encode};

fn config_for(input: &AudioInput, max_lpc_order: u8) -> StreamEncoderConfig {
    StreamEncoderConfig {
        sample_rate: input.sample_rate,
        channels: input.channels,
        bits_per_sample: input.bits_per_sample,
        block_size: input.block_size,
        max_lpc_order,
        ..StreamEncoderConfig::default()
    }
}

proptest! {
    /// 任意の入力でエンコード → デコードがロスレスである
    #[test]
    fn roundtrip_is_lossless(input in audio_input()) {
        let config = config_for(&input, 8);
        let encoded = encode(config, &input.samples).expect("エンコードに成功するはず");
        // デコーダーは MD5 と総サンプル数の検証まで行う
        let decoded = decoder::decode(&encoded).expect("デコードに成功するはず");
        prop_assert_eq!(&decoded.samples, &input.samples, "ロスレス性が壊れている");
        prop_assert_eq!(decoded.channels, input.channels);
        prop_assert_eq!(decoded.bits_per_sample, input.bits_per_sample);
        prop_assert_eq!(decoded.sample_rate, input.sample_rate);
    }

    /// ステレオデコリレーション経路もロスレスである
    #[test]
    fn stereo_decorrelation_is_lossless(input in stereo_input()) {
        let config = config_for(&input, 8);
        let encoded = encode(config, &input.samples).expect("エンコードに成功するはず");
        let decoded = decoder::decode(&encoded).expect("デコードに成功するはず");
        prop_assert_eq!(&decoded.samples, &input.samples, "ロスレス性が壊れている");
    }

    /// LPC を無効にしてもロスレスである (固定予測のみの経路)
    #[test]
    fn roundtrip_without_lpc_is_lossless(input in audio_input()) {
        let config = config_for(&input, 0);
        let encoded = encode(config, &input.samples).expect("エンコードに成功するはず");
        let decoded = decoder::decode(&encoded).expect("デコードに成功するはず");
        prop_assert_eq!(&decoded.samples, &input.samples, "ロスレス性が壊れている");
    }

    /// サンプルをどのように分割して push しても一括 push と同じ出力になる
    #[test]
    fn streaming_push_matches_oneshot(
        input in stereo_input(),
        chunk_size in 1usize..=97,
    ) {
        let oneshot = encode(config_for(&input, 8), &input.samples)
            .expect("エンコードに成功するはず");

        let mut encoder = StreamEncoder::new(config_for(&input, 8))
            .expect("エンコーダーの作成に成功するはず");
        // チャンネル数の倍数に切り上げた chunk で分割投入する
        let step = chunk_size * usize::from(input.channels);
        for chunk in input.samples.chunks(step) {
            encoder.push_samples(chunk).expect("push に成功するはず");
        }
        let streamed = encoder.finish().expect("finish に成功するはず");
        prop_assert_eq!(oneshot, streamed);
    }
}
