//! PBT テスト共通ユーティリティ

use proptest::prelude::*;

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

/// ビット深度に収まるサンプル 1 個の strategy
pub fn sample_value(bits_per_sample: u8) -> impl Strategy<Value = i32> {
    let low = -(1i64 << (bits_per_sample - 1));
    let high = (1i64 << (bits_per_sample - 1)) - 1;
    (low..=high).prop_map(|v| v as i32)
}

/// 任意のオーディオ入力 (チャンネル数・ビット深度・サンプル列) の strategy
///
/// ブロックサイズを小さくして複数フレームにまたがるケースを効率よく生成する。
pub fn audio_input() -> impl Strategy<Value = AudioInput> {
    (1u8..=8, 4u8..=32, 1u32..=0xF_FFFF, 16u16..=64, 0usize..=200).prop_flat_map(
        |(channels, bits_per_sample, sample_rate, block_size, frames)| {
            let count = frames * usize::from(channels);
            proptest::collection::vec(sample_value(bits_per_sample), count).prop_map(
                move |samples| AudioInput {
                    channels,
                    bits_per_sample,
                    sample_rate,
                    block_size,
                    samples,
                },
            )
        },
    )
}

/// ステレオ 16 bit に限定したオーディオ入力の strategy
///
/// ステレオデコリレーション (mid-side 等) の経路を重点的に生成する。
pub fn stereo_input() -> impl Strategy<Value = AudioInput> {
    (16u16..=64, 0usize..=200).prop_flat_map(|(block_size, frames)| {
        proptest::collection::vec(sample_value(16), frames * 2).prop_map(move |samples| {
            AudioInput {
                channels: 2,
                bits_per_sample: 16,
                sample_rate: 44100,
                block_size,
                samples,
            }
        })
    })
}
