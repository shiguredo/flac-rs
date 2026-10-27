//! encoder モジュールの PBT
//!
//! FLAC の根幹であるロスレス性 (エンコード → デコードで元のサンプル列と
//! 完全一致) を任意の入力に対して検証する。

use pbt::{AudioInput, audio_input, stereo_input};
use shiguredo_flac::decoder;
use shiguredo_flac::encoder::{StreamEncoder, StreamEncoderConfig, encode};

/// テストケース数
///
/// エンコードとデコードを 1 ケースで 2 回ずつ回すため、1 ケースのコストが
/// 高い。探索空間は有限で棄却もないため、この数で十分に全クラスを踏む。
const CASES: usize = 128;

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

/// 任意の入力でエンコード → デコードがロスレスである
#[test]
fn roundtrip_is_lossless() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let input = audio_input(ctx);
        let config = config_for(&input, 8);
        let encoded = encode(config, &input.samples).expect("エンコードに成功するはず");
        // デコーダーは MD5 と総サンプル数の検証まで行う
        let decoded = decoder::decode(&encoded).expect("デコードに成功するはず");
        assert_eq!(
            decoded.samples,
            input.samples,
            "ロスレス性が壊れている (channels={}, bits={}, block_size={}, samples={})",
            input.channels,
            input.bits_per_sample,
            input.block_size,
            input.samples.len(),
        );
        assert_eq!(decoded.channels, input.channels);
        assert_eq!(decoded.bits_per_sample, input.bits_per_sample);
        assert_eq!(decoded.sample_rate, input.sample_rate);
        Ok(())
    })?;

    Ok(())
}

/// ステレオデコリレーション経路もロスレスである
#[test]
fn stereo_decorrelation_is_lossless() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let input = stereo_input(ctx);
        let config = config_for(&input, 8);
        let encoded = encode(config, &input.samples).expect("エンコードに成功するはず");
        let decoded = decoder::decode(&encoded).expect("デコードに成功するはず");
        assert_eq!(
            decoded.samples,
            input.samples,
            "ステレオデコリレーション経路でロスレス性が壊れている (block_size={}, samples={})",
            input.block_size,
            input.samples.len(),
        );
        Ok(())
    })?;

    Ok(())
}

/// LPC を無効にしてもロスレスである (固定予測のみの経路)
#[test]
fn roundtrip_without_lpc_is_lossless() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let input = audio_input(ctx);
        let config = config_for(&input, 0);
        let encoded = encode(config, &input.samples).expect("エンコードに成功するはず");
        let decoded = decoder::decode(&encoded).expect("デコードに成功するはず");
        assert_eq!(
            decoded.samples,
            input.samples,
            "LPC 無効時にロスレス性が壊れている (channels={}, bits={}, samples={})",
            input.channels,
            input.bits_per_sample,
            input.samples.len(),
        );
        Ok(())
    })?;

    Ok(())
}

/// サンプルをどのように分割して push しても一括 push と同じ出力になる
#[test]
fn streaming_push_matches_oneshot() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    // 複数チャンクへの分割が実際に起きたことを確認するゲート
    let split_cases = std::cell::Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let input = stereo_input(ctx);
        let chunk_size = noprop::sample_usize_in(ctx, 1..=97);

        let oneshot =
            encode(config_for(&input, 8), &input.samples).expect("エンコードに成功するはず");

        let mut encoder =
            StreamEncoder::new(config_for(&input, 8)).expect("エンコーダーの作成に成功するはず");
        // チャンネル数の倍数に切り上げた chunk で分割投入する
        let step = chunk_size * usize::from(input.channels);
        if input.samples.len() > step {
            split_cases.set(split_cases.get() + 1);
        }
        for chunk in input.samples.chunks(step) {
            encoder.push_samples(chunk).expect("push に成功するはず");
        }
        let streamed = encoder.finish().expect("finish に成功するはず");
        assert_eq!(
            streamed,
            oneshot,
            "分割投入の結果が一括投入と異なる (chunk_size={}, samples={})",
            chunk_size,
            input.samples.len(),
        );
        Ok(())
    })?;

    // 全ケースが 1 チャンクで収まると分割経路を一度も踏まないまま成功してしまう
    assert!(
        split_cases.get() > 0,
        "分割投入が一度も発生しなかった\n{runner}"
    );

    Ok(())
}
