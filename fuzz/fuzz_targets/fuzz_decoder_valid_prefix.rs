//! 正しい FLAC ストリームの破損に対するデコーダーのパニック安全性を検証する
//!
//! 完全にランダムな入力は fLaC マーカーで早期に弾かれてしまい、フレーム
//! デコードの深部に到達しにくい。ここでは有効な FLAC ストリームを生成して
//! から任意のバイトを破壊し、深い経路でのパニック安全性を検証する。

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_flac::decoder::StreamDecoder;
use shiguredo_flac::encoder::{StreamEncoderConfig, encode};

fuzz_target!(|input: (Vec<i32>, Vec<(u16, u8)>)| {
    let (raw_samples, corruptions) = input;

    // 有効な FLAC ストリームを作る (16 bit ステレオ)
    let samples: Vec<i32> = raw_samples
        .iter()
        .map(|&s| i64::from(s).clamp(-32768, 32767) as i32)
        .collect();
    let samples = &samples[..samples.len() / 2 * 2];
    let config = StreamEncoderConfig {
        block_size: 64,
        ..StreamEncoderConfig::default()
    };
    let mut flac_bytes = encode(config, samples).expect("有効な入力のエンコードは成功するはず");

    // 任意の位置のバイトを破壊する
    for &(position, value) in &corruptions {
        let len = flac_bytes.len();
        flac_bytes[usize::from(position) % len] ^= value;
    }

    // 破損したストリームでもパニックしない (エラーは正常系)
    let mut decoder = StreamDecoder::new();
    decoder.feed(&flac_bytes);
    decoder.finish();
    while let Ok(Some(_frame)) = decoder.decode_frame() {}
});
