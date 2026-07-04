//! 任意バイト列に対するデコーダーのパニック安全性を検証する
//!
//! - 任意の入力を一括で feed してデコードする
//! - パニック・メモリ違反・無限ループが起きないことだけを確認する
//!   (エラーになるのは正常な動作)

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_flac::decoder::StreamDecoder;

fuzz_target!(|data: &[u8]| {
    let mut decoder = StreamDecoder::new();
    decoder.feed(data);
    decoder.finish();
    // フレームが取り出せる限り取り出す。エラーは正常系
    while let Ok(Some(_frame)) = decoder.decode_frame() {}
    let _ = decoder.stream_info();
    let _ = decoder.metadata();
});
