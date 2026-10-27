//! PBT テスト共通ユーティリティ
//!
//! noprop のサンプラーを使って、エンコーダー設定とサンプル列の組を生成する。
//! 生成する値はすべて「有効な入力」になるように構成的に組み立てるため、
//! 生成側での棄却 (reject) は行わない。

/// エンコーダー設定とサンプル列の組
#[derive(Debug, Clone)]
pub struct AudioInput {
    /// チャンネル数 (1-8)
    pub channels: u8,
    /// サンプルあたりのビット数 (4-32)
    pub bits_per_sample: u8,
    /// サンプルレート (Hz)
    pub sample_rate: u32,
    /// ブロックサイズ
    pub block_size: u16,
    /// インターリーブ済みサンプル列 (チャンネル数の倍数)
    pub samples: Vec<i32>,
}

/// ビット深度に収まるサンプル 1 個を生成する
///
/// 範囲は `-(2^(bits-1))..=2^(bits-1)-1`。`bits_per_sample` は 1..=32 を
/// 想定しており、それ以外を渡すとパニックする。
pub fn sample_value(ctx: &mut noprop::TestCaseContext, bits_per_sample: u8) -> i32 {
    let half = 1i64 << (bits_per_sample - 1);
    let low = -half;
    let high = half - 1;
    // ビット深度 32 では範囲が i32 の全域になり `sample_i32_in` が使えないため、
    // i32 の一様乱数を i64 に拡張してからサンプル範囲へ写す
    // (span は 2 の冪なので剰余写像に偏りは生じない)
    let offset = i64::from(noprop::sample_i32(ctx));
    let span = high - low + 1;
    (low + offset.rem_euclid(span)) as i32
}

/// チャンネル数を生成する
///
/// モノラル / ステレオ / 最大 8 チャンネルを境界値として確実に踏む。
fn sample_channels(ctx: &mut noprop::TestCaseContext) -> u8 {
    noprop::sample_with_boundaries(ctx, &[1u8, 2, 8], noprop::Ratio::new(1, 2), |ctx| {
        noprop::sample_u8_in(ctx, 1..=8)
    })
}

/// ビット深度を生成する
///
/// 最小 4 / 8 / 16 / 32 bit を境界値として確実に踏む。
fn sample_bits_per_sample(ctx: &mut noprop::TestCaseContext) -> u8 {
    noprop::sample_with_boundaries(ctx, &[4u8, 8, 16, 32], noprop::Ratio::new(1, 2), |ctx| {
        noprop::sample_u8_in(ctx, 4..=32)
    })
}

/// ブロックサイズを生成する
///
/// 最小 16 / 最大 64 を境界値として確実に踏む。
fn sample_block_size(ctx: &mut noprop::TestCaseContext) -> u16 {
    noprop::sample_with_boundaries(ctx, &[16u16, 64], noprop::Ratio::new(1, 2), |ctx| {
        noprop::sample_u16_in(ctx, 16..=64)
    })
}

/// フレーム数 (1 チャンネルあたりのサンプル数) を生成する
///
/// 空ストリーム (0) と長めのストリーム (200) を境界値として確実に踏む。
fn sample_frames(ctx: &mut noprop::TestCaseContext) -> usize {
    noprop::sample_with_boundaries(ctx, &[0usize, 1, 200], noprop::Ratio::new(1, 4), |ctx| {
        noprop::sample_usize_in(ctx, 0..=200)
    })
}

/// 任意のオーディオ入力 (チャンネル数・ビット深度・サンプル列) を生成する
///
/// ブロックサイズを小さくして複数フレームにまたがるケースを効率よく生成する。
pub fn audio_input(ctx: &mut noprop::TestCaseContext) -> AudioInput {
    let channels = sample_channels(ctx);
    let bits_per_sample = sample_bits_per_sample(ctx);
    let sample_rate = noprop::sample_u32_in(ctx, 1..=0xF_FFFF);
    let block_size = sample_block_size(ctx);
    let frames = sample_frames(ctx);
    let count = frames * usize::from(channels);
    let samples = (0..count)
        .map(|_| sample_value(ctx, bits_per_sample))
        .collect();
    AudioInput {
        channels,
        bits_per_sample,
        sample_rate,
        block_size,
        samples,
    }
}

/// ステレオ 16 bit に限定したオーディオ入力を生成する
///
/// ステレオデコリレーション (mid-side 等) の経路を重点的に生成する。
pub fn stereo_input(ctx: &mut noprop::TestCaseContext) -> AudioInput {
    let block_size = sample_block_size(ctx);
    let frames = sample_frames(ctx);
    let samples = (0..frames * 2).map(|_| sample_value(ctx, 16)).collect();
    AudioInput {
        channels: 2,
        bits_per_sample: 16,
        sample_rate: 44100,
        block_size,
        samples,
    }
}
