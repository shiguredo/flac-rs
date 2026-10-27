//! decoder モジュールの PBT
//!
//! Sans I/O デコーダーの再開性 (どのように分割して feed しても結果が同じ) を
//! 検証する。

use pbt::{AudioInput, audio_input};
use proptest::prelude::*;
use shiguredo_flac::decoder::{StreamDecoder, decode};
use shiguredo_flac::encoder::{StreamEncoderConfig, encode};

fn encode_input(input: &AudioInput) -> Vec<u8> {
    let config = StreamEncoderConfig {
        sample_rate: input.sample_rate,
        channels: input.channels,
        bits_per_sample: input.bits_per_sample,
        block_size: input.block_size,
        ..StreamEncoderConfig::default()
    };
    encode(config, &input.samples).expect("エンコードに成功するはず")
}

proptest! {
    /// 任意の分割で feed しても一括デコードと同じ結果になる
    #[test]
    fn chunked_feed_matches_oneshot(
        input in audio_input(),
        chunk_size in 1usize..=blessed_max_chunk(),
    ) {
        let flac_bytes = encode_input(&input);
        let oneshot = decode(&flac_bytes).expect("一括デコードに成功するはず");

        let mut decoder = StreamDecoder::new();
        let mut samples = Vec::new();
        for chunk in flac_bytes.chunks(chunk_size) {
            decoder.feed(chunk);
            while let Some(frame) = decoder.decode_frame().expect("デコードに成功するはず") {
                samples.extend_from_slice(&frame.samples);
            }
        }
        decoder.finish();
        while let Some(frame) = decoder.decode_frame().expect("デコードに成功するはず") {
            samples.extend_from_slice(&frame.samples);
        }
        prop_assert_eq!(samples, oneshot.samples);
        prop_assert_eq!(decoder.metadata(), &oneshot.metadata[..]);
    }

    /// feed 完了後の繰り返し decode_frame 呼び出しは安全に None を返し続ける
    #[test]
    fn decode_after_end_returns_none(input in audio_input()) {
        let flac_bytes = encode_input(&input);
        let mut decoder = StreamDecoder::new();
        decoder.feed(&flac_bytes);
        decoder.finish();
        while decoder.decode_frame().expect("デコードに成功するはず").is_some() {}
        // 終端後は何度呼んでも None
        for _ in 0..3 {
            prop_assert_eq!(decoder.decode_frame().expect("終端後も成功するはず"), None);
        }
    }

    /// メタデータの読み終え判定が、実際にメタデータが揃った時点と一致する
    ///
    /// `metadata_complete()` が true を返した時点で `metadata()` の内容は
    /// 最終的なものになっており、その後フレームを読み進めても変化しない
    #[test]
    fn metadata_complete_stabilizes_metadata(
        input in audio_input(),
        chunk_size in 1usize..=blessed_max_chunk(),
    ) {
        let flac_bytes = encode_input(&input);
        let mut decoder = StreamDecoder::new();

        let mut completed_metadata: Option<Vec<_>> = None;
        for chunk in flac_bytes.chunks(chunk_size) {
            decoder.feed(chunk);
            // フレームが返らない (データ不足) 場合もメタデータの進行は進むため、
            // decode_frame のたびに判定する
            loop {
                let frame = decoder.decode_frame().expect("デコードに成功するはず");
                // 読み終え判定が true になった最初の時点で内容を控える
                if decoder.metadata_complete() && completed_metadata.is_none() {
                    completed_metadata = Some(decoder.metadata().to_vec());
                }
                // 読み終えた後に内容が変わることはない
                if let Some(recorded) = &completed_metadata {
                    prop_assert_eq!(decoder.metadata(), &recorded[..]);
                }
                if frame.is_none() {
                    break;
                }
            }
        }
        decoder.finish();
        while decoder.decode_frame().expect("デコードに成功するはず").is_some() {}

        // ストリーム全体を読み終えた時点では必ず true
        prop_assert!(decoder.metadata_complete());
        // ストリームにフレームがあれば、最初のフレームをデコードした時点で
        // メタデータは確定している
        if !input.samples.is_empty() {
            let recorded = completed_metadata.expect("最初のフレームでメタデータが確定するはず");
            prop_assert_eq!(decoder.metadata(), &recorded[..]);
        } else {
            // フレームが 1 つもないストリームでも、終端まで読めば確定する
            prop_assert_eq!(decoder.stream_info(), Some(&match decoder.metadata().first() {
                Some(shiguredo_flac::metadata::MetadataBlock::StreamInfo(info)) => info.clone(),
                _ => panic!("最初のメタデータブロックは STREAMINFO のはず"),
            }));
        }
    }
}

/// chunk_size strategy の上限
///
/// 定数だが proptest のマクロ内で読みやすいよう関数にしている。
fn blessed_max_chunk() -> usize {
    257
}
