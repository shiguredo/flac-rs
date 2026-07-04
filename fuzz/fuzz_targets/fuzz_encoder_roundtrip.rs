//! 任意サンプル列に対するエンコーダーのロスレス性を検証する
//!
//! - 任意のチャンネル数・ビット深度・サンプル列でエンコードする
//! - デコードして元のサンプル列と完全一致することを確認する

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_flac::decoder;
use shiguredo_flac::encoder::{StreamEncoderConfig, encode};

fuzz_target!(|input: (u8, u8, u8, Vec<i32>)| {
    let (channels, bits_per_sample, lpc_order, raw_samples) = input;
    let channels = channels % 8 + 1;
    let bits_per_sample = bits_per_sample % 29 + 4;
    let max_lpc_order = lpc_order % 33;

    // サンプルをビット深度の範囲に丸め、チャンネル数の倍数に切り詰める
    let low = -(1i64 << (bits_per_sample - 1));
    let high = (1i64 << (bits_per_sample - 1)) - 1;
    let mut samples: Vec<i32> = raw_samples
        .into_iter()
        .map(|s| i64::from(s).clamp(low, high) as i32)
        .collect();
    samples.truncate(samples.len() / usize::from(channels) * usize::from(channels));

    let config = StreamEncoderConfig {
        channels,
        bits_per_sample,
        max_lpc_order,
        block_size: 64,
        ..StreamEncoderConfig::default()
    };
    let encoded = encode(config, &samples).expect("有効な入力のエンコードは成功するはず");
    let decoded = decoder::decode(&encoded).expect("エンコード結果のデコードは成功するはず");
    assert_eq!(decoded.samples, samples, "ロスレス性が壊れている");
});
