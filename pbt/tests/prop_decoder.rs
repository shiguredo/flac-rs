//! decoder モジュールの PBT
//!
//! Sans I/O デコーダーの再開性 (どのように分割して feed しても結果が同じ) を
//! 検証する。

use pbt::{AudioInput, audio_input};
use shiguredo_flac::decoder::{StreamDecoder, decode};
use shiguredo_flac::encoder::{StreamEncoderConfig, encode};

/// テストケース数
const CASES: usize = 128;

/// chunk_size の上限
///
/// 1 バイト刻みからストリーム全体まで、分割の粒度を幅広く踏む。
const MAX_CHUNK: usize = 257;

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

/// 任意の分割で feed しても一括デコードと同じ結果になる
#[test]
fn chunked_feed_matches_oneshot() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    // 複数チャンクへの分割が実際に起きたことを確認するゲート
    let split_cases = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let input = audio_input(ctx);
        let chunk_size = noprop::sample_usize_in(ctx, 1..=MAX_CHUNK);

        let flac_bytes = encode_input(&input);
        let oneshot = decode(&flac_bytes).expect("一括デコードに成功するはず");

        if flac_bytes.len() > chunk_size {
            split_cases.set(split_cases.get() + 1);
        }

        let mut decoder = StreamDecoder::new();
        let mut samples = Vec::new();
        for chunk in flac_bytes.chunks(chunk_size) {
            decoder.feed(chunk);
            while let Some(frame) = decoder.decode_frame().expect("デコードに成功するはず")
            {
                samples.extend_from_slice(&frame.samples);
            }
        }
        decoder.finish();
        while let Some(frame) = decoder.decode_frame().expect("デコードに成功するはず") {
            samples.extend_from_slice(&frame.samples);
        }
        assert_eq!(
            samples, oneshot.samples,
            "分割 feed のサンプル列が一括デコードと異なる (chunk_size={})",
            chunk_size,
        );
        assert_eq!(
            decoder.metadata(),
            &oneshot.metadata[..],
            "分割 feed のメタデータが一括デコードと異なる (chunk_size={})",
            chunk_size,
        );
        Ok(())
    })?;

    // 全ケースが 1 チャンクで収まると分割 feed の性質が空振りしてしまう
    assert!(
        split_cases.get() > 0,
        "複数チャンクへの分割が一度も発生しなかった\n{runner}"
    );

    Ok(())
}

/// feed 完了後の繰り返し decode_frame 呼び出しは安全に None を返し続ける
#[test]
fn decode_after_end_returns_none() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let input = audio_input(ctx);
        let flac_bytes = encode_input(&input);

        let mut decoder = StreamDecoder::new();
        decoder.feed(&flac_bytes);
        decoder.finish();
        while decoder
            .decode_frame()
            .expect("デコードに成功するはず")
            .is_some()
        {}
        // 終端後は何度呼んでも None
        for i in 0..3 {
            assert_eq!(
                decoder.decode_frame().expect("終端後も成功するはず"),
                None,
                "終端後の decode_frame が {} 回目で None 以外を返した",
                i + 1,
            );
        }
        Ok(())
    })?;

    Ok(())
}

/// メタデータの読み終え判定が、実際にメタデータが揃った時点と一致する
///
/// `metadata_complete()` が true を返した時点で `metadata()` の内容は
/// 最終的なものになっており、その後フレームを読み進めても変化しない
#[test]
fn metadata_complete_stabilizes_metadata() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    // STREAMINFO が実際に読めたケースがあったことを確認するゲート
    let with_stream_info = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let input = audio_input(ctx);
        let chunk_size = noprop::sample_usize_in(ctx, 1..=MAX_CHUNK);

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
                    assert_eq!(
                        decoder.metadata(),
                        &recorded[..],
                        "メタデータ読み終え後に内容が変化した (chunk_size={})",
                        chunk_size,
                    );
                }
                if frame.is_none() {
                    break;
                }
            }
        }
        decoder.finish();
        while decoder
            .decode_frame()
            .expect("デコードに成功するはず")
            .is_some()
        {}

        // ストリーム全体を読み終えた時点では必ず true
        assert!(
            decoder.metadata_complete(),
            "ストリーム全体を読み終えても metadata_complete() が false のまま"
        );
        // ストリームにフレームがあれば、最初のフレームをデコードした時点で
        // メタデータは確定している
        if !input.samples.is_empty() {
            let recorded = completed_metadata.expect("最初のフレームでメタデータが確定するはず");
            assert_eq!(
                decoder.metadata(),
                &recorded[..],
                "フレーム読み進め後にメタデータが変化した",
            );
        } else {
            // フレームが 1 つもないストリームでも、終端まで読めば確定する
            let Some(shiguredo_flac::metadata::MetadataBlock::StreamInfo(info)) =
                decoder.metadata().first()
            else {
                panic!("最初のメタデータブロックは STREAMINFO のはず");
            };
            assert_eq!(decoder.stream_info(), Some(info));
        }
        if decoder.stream_info().is_some() {
            with_stream_info.set(with_stream_info.get() + 1);
        }
        Ok(())
    })?;

    // 全ケースで STREAMINFO が読めないと、この性質が空振りして成功し続ける
    assert!(
        with_stream_info.get() > 0,
        "STREAMINFO を取得できたケースが 1 件もなかった\n{runner}"
    );

    Ok(())
}
