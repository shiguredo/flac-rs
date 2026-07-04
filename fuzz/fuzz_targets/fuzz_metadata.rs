//! 任意バイト列に対するメタデータブロックデコードのパニック安全性を検証する
//!
//! - 任意のブロックタイプ + ペイロードをデコードする
//! - デコードに成功した場合は再エンコードして一致することを確認する
//!   (デコード → エンコードの正規形ラウンドトリップ)

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_flac::metadata::MetadataBlock;

fuzz_target!(|input: (u8, &[u8])| {
    let (block_type, payload) = input;
    if let Ok(block) = MetadataBlock::decode(block_type & 0x7F, payload) {
        // デコードできたブロックは必ず再エンコードできる
        let reencoded = block
            .encode_payload()
            .expect("デコードできたブロックは再エンコードできるはず");
        // PADDING は中身を保持しない (全て 0 になる) ため除外し、
        // それ以外はバイト列が一致する
        if !matches!(block, MetadataBlock::Padding { .. }) {
            assert_eq!(reencoded, payload);
        } else {
            assert_eq!(reencoded.len(), payload.len());
        }
    }
});
