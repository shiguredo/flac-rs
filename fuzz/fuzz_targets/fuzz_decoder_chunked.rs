//! 分割 feed に対するデコーダーのパニック安全性と一括デコードとの等価性を検証する
//!
//! - 任意の入力を任意のチャンクサイズで feed してデコードする
//! - 一括 feed した場合と同じ結果 (成功/失敗、フレーム数) になることを確認する

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_flac::decoder::StreamDecoder;

fuzz_target!(|input: (u8, &[u8])| {
    let (chunk_size, data) = input;
    let chunk_size = usize::from(chunk_size).max(1);

    // 一括 feed
    let mut oneshot = StreamDecoder::new();
    oneshot.feed(data);
    oneshot.finish();
    let mut oneshot_frames = Vec::new();
    let oneshot_result = loop {
        match oneshot.decode_frame() {
            Ok(Some(frame)) => oneshot_frames.push(frame),
            Ok(None) => break Ok(()),
            Err(e) => break Err(e),
        }
    };

    // 分割 feed
    let mut chunked = StreamDecoder::new();
    let mut chunked_frames = Vec::new();
    let mut chunked_result = Ok(());
    'outer: {
        for chunk in data.chunks(chunk_size) {
            chunked.feed(chunk);
            loop {
                match chunked.decode_frame() {
                    Ok(Some(frame)) => chunked_frames.push(frame),
                    Ok(None) => break,
                    Err(e) => {
                        chunked_result = Err(e);
                        break 'outer;
                    }
                }
            }
        }
        chunked.finish();
        loop {
            match chunked.decode_frame() {
                Ok(Some(frame)) => chunked_frames.push(frame),
                Ok(None) => break,
                Err(e) => {
                    chunked_result = Err(e);
                    break;
                }
            }
        }
    }

    // 分割してもフレーム列と結果が変わらない
    assert_eq!(oneshot_result, chunked_result);
    assert_eq!(oneshot_frames, chunked_frames);
});
