//! impulse 複製信号の照合テスト
//!
//! 生成コードの複製が元から乖離すると圧縮率の比較自体の意味がなくなるため、
//! 期待値 (flac_compare 側の impulse から一度生成した固定値) と照合する。

use benches::impulse_signal;

/// impulse 複製の生成結果が flac_compare の signal::impulse と一致する
///
/// 全サンプル列を埋め込む代わりに、サンプル数・スパイク数・
/// FNV-1a 64 ハッシュで完全一致を照合する。
#[test]
fn impulse_signal_matches_reference_impl() {
    const IMPULSE_FRAMES: usize = 44100 * 5;
    let samples = impulse_signal(IMPULSE_FRAMES);
    // サンプル数 (インターリーブ済み) とスパイク数
    assert_eq!(samples.len(), 441000, "サンプル数が一致すること");
    let spikes = samples.iter().step_by(2).filter(|&&v| v != 0).count();
    assert_eq!(spikes, 394, "スパイク数が一致すること");
    // FNV-1a 64 ハッシュ (符号付き i32 を 0x8000_0000 でオフセットして
    // 符号なしに変換してから混ぜる。オフセットは全単射なので、ハッシュ一致は
    // サンプル列の完全一致を意味する)
    let mut hash: u64 = 0xcbf29ce484222325;
    for &sample in &samples {
        hash ^= (sample as i64 as u64).wrapping_add(0x8000_0000);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    assert_eq!(
        hash, 0xaceb5e5567101cf1,
        "FNV-1a 64 ハッシュが flac_compare の impulse と一致すること"
    );
}
