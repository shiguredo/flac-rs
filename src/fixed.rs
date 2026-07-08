//! 固定予測 (RFC 9639 Section 9.2.5)
//!
//! 次数 0 から 4 の固定 (定義済み) 予測器。予測器係数を格納する必要がないため、
//! 単純な波形の予測に適する。
//!
//! 全ての算術は `i64` で行う。サブフレームのサンプルは最大 33 bit
//! (32 bit + サイドチャンネルの 1 bit) で、次数 4 の予測は最大 4 bit 増える
//! ため 37 bit に収まり、オーバーフローしない (RFC 9639 Appendix A.3)。
//!
//! ただしこの保証は全サンプルがビット深度に収まっている場合のもの。壊れた
//! ストリームでは復元サンプルが逐次的に増大し得るため、デコード時は復元した
//! サンプルごとにビット深度の範囲を検証し、範囲外を即座に不正データとして
//! 拒否する (RFC 9639 Section 5)。

use alloc::format;
use alloc::vec::Vec;

use crate::error::{DecodeError, ParseError};

/// 固定予測の最大次数 (RFC 9639 Section 9.2.5)
pub(crate) const MAX_FIXED_ORDER: usize = 4;

/// 固定予測の残差を計算する (エンコード)
///
/// `samples` の先頭 `order` 個は warm-up サンプルとして扱い、残りについて
/// 残差 (サンプル値 - 予測値) を返す。
///
/// サンプルごとに次数の分岐が入らないよう、次数ごとの専用ループで計算する。
/// 各次数の残差は過去サンプルの二項係数による線形結合
/// (RFC 9639 Appendix A.3 Table 26)。
pub(crate) fn compute_residual(samples: &[i64], order: usize, residual: &mut Vec<i64>) {
    debug_assert!(order <= MAX_FIXED_ORDER, "固定予測の次数は 0-4 (実装バグ)");
    debug_assert!(samples.len() >= order, "サンプル数が次数未満 (実装バグ)");
    // windows + extend は長さが既知のイテレータになり、要素ごとの容量
    // チェックを伴わない (自動ベクトル化しやすい形)。バッファは呼び出し側が
    // 使い回すため、確保は初回のみ
    residual.clear();
    residual.reserve(samples.len() - order);
    match order {
        0 => residual.extend_from_slice(samples),
        1 => residual.extend(samples.windows(2).map(|w| w[1] - w[0])),
        2 => residual.extend(samples.windows(3).map(|w| w[2] - 2 * w[1] + w[0])),
        3 => residual.extend(
            samples
                .windows(4)
                .map(|w| w[3] - 3 * w[2] + 3 * w[1] - w[0]),
        ),
        _ => residual.extend(
            samples
                .windows(5)
                .map(|w| w[4] - 4 * w[3] + 6 * w[2] - 4 * w[1] + w[0]),
        ),
    }
}

/// 固定予測の残差を計算する (エンコード、i32 格納版)
///
/// `compute_residual` と同じ計算を i32 格納のサンプル列に対して行う。演算は
/// `i64::from` で widening してから行うので、結果は i64 版とビット単位で一致
/// する。i32 の連続ロード + widening 減算は自動ベクトル化される (i64 格納の
/// 2 倍のレーン幅)。
pub(crate) fn compute_residual_i32(samples: &[i32], order: usize, residual: &mut Vec<i64>) {
    debug_assert!(order <= MAX_FIXED_ORDER, "固定予測の次数は 0-4 (実装バグ)");
    debug_assert!(samples.len() >= order, "サンプル数が次数未満 (実装バグ)");
    // windows + extend は長さが既知のイテレータになり、要素ごとの容量
    // チェックを伴わない。バッファは呼び出し側が使い回すため、確保は初回のみ
    residual.clear();
    residual.reserve(samples.len() - order);
    match order {
        0 => residual.extend(samples.iter().map(|&s| i64::from(s))),
        1 => residual.extend(
            samples
                .windows(2)
                .map(|w| i64::from(w[1]) - i64::from(w[0])),
        ),
        2 => residual.extend(
            samples
                .windows(3)
                .map(|w| i64::from(w[2]) - 2 * i64::from(w[1]) + i64::from(w[0])),
        ),
        3 => residual.extend(samples.windows(4).map(|w| {
            i64::from(w[3]) - 3 * i64::from(w[2]) + 3 * i64::from(w[1]) - i64::from(w[0])
        })),
        _ => residual.extend(samples.windows(5).map(|w| {
            i64::from(w[4]) - 4 * i64::from(w[3]) + 6 * i64::from(w[2]) - 4 * i64::from(w[1])
                + i64::from(w[0])
        })),
    }
}

/// 全次数 (0-4) の残差絶対値和を 1 パスで計算し、最小の次数を返す
///
/// 次数ごとに `compute_residual` を呼ぶと残差の生成とメモリ確保が 5 回
/// 走るため、次数選択は誤差和だけを 1 パスで求める (リファレンス実装
/// libFLAC の FLAC__fixed_compute_best_predictor と同じ考え方)。
/// 先頭 4 サンプルは全次数共通で評価から除くが、次数選択のヒューリスティック
/// としては十分な精度がある。
pub(crate) fn best_order(samples: &[i64], max_order: usize) -> usize {
    debug_assert!(
        max_order <= MAX_FIXED_ORDER,
        "固定予測の次数は 0-4 (実装バグ)"
    );
    if samples.len() <= 4 {
        // 評価対象のサンプルがない。全次数同点として次数 0 を返す
        return 0;
    }
    // 次数 k の残差は次数 k-1 の残差の階差に等しいため、乗算を使う二項係数の
    // 線形結合 (RFC 9639 Appendix A.3 Table 26) と同じ値を減算 4 回の
    // カスケードで求める (リファレンス実装 libFLAC と同じ形)。
    // サンプルは 33 bit 以内なので残差は 37 bit 以内、総和は 65535 サンプル
    // でも u64 に収まる
    let mut totals = [0u64; MAX_FIXED_ORDER + 1];
    let mut prev_e0 = samples[3];
    let mut prev_e1 = samples[3] - samples[2];
    let mut prev_e2 = prev_e1 - (samples[2] - samples[1]);
    let mut prev_e3 = prev_e2 - ((samples[2] - samples[1]) - (samples[1] - samples[0]));
    for &sample in &samples[4..] {
        let e1 = sample - prev_e0;
        let e2 = e1 - prev_e1;
        let e3 = e2 - prev_e2;
        let e4 = e3 - prev_e3;
        totals[0] += sample.unsigned_abs();
        totals[1] += e1.unsigned_abs();
        totals[2] += e2.unsigned_abs();
        totals[3] += e3.unsigned_abs();
        totals[4] += e4.unsigned_abs();
        prev_e0 = sample;
        prev_e1 = e1;
        prev_e2 = e2;
        prev_e3 = e3;
    }
    // 同点では低い次数 (warm-up が少なくヘッダーが単純な方) を選ぶ
    let mut best = 0;
    for (order, &total) in totals.iter().enumerate().take(max_order + 1).skip(1) {
        if total < totals[best] {
            best = order;
        }
    }
    best
}

/// `best_order_i32` を使ってよいビット深度の上限
///
/// 階差 k 段の最大絶対値は 2^(bits-1) の 2^k 倍に膨らむ。次数 4 まで i32 の
/// 演算で溢れないためには bits + 4 ≤ 31、つまり bits ≤ 27 が必要
/// (RFC 9639 Appendix A.3)。
pub(crate) const BEST_ORDER_I32_MAX_BITS: u32 = 27;

/// 全次数 (0-4) の残差絶対値和を 1 パスで計算し、最小の次数を返す (i32 格納版)
///
/// ビット深度が `BEST_ORDER_I32_MAX_BITS` 以下のサンプル列にだけ使うこと
/// (16 bit / 24 bit 音源はサイドチャンネルを含めてこの範囲に入る)。
///
/// `best_order` の持ち回りカスケード (直前の階差をレジスタで引き継ぐ形) は
/// ループ間の逐次依存があり自動ベクトル化されない。i32 格納版は 5 点窓の中で
/// 階差を組み立て直す形にする。窓ごとの計算は独立で、中間値も全て i32 に
/// 収まるため、i32 の 4 レーンで自動ベクトル化される (中間を i64 に widening
/// すると 2 レーンに落ち、演算数の増加を吸収できない。実測)。窓内カスケードは
/// 二項係数の線形結合 (RFC 9639 Appendix A.3 Table 26) と数学的に同一で、
/// `best_order` と同じ次数を返す (乗算も使わない)。
pub(crate) fn best_order_i32(samples: &[i32], max_order: usize) -> usize {
    debug_assert!(
        max_order <= MAX_FIXED_ORDER,
        "固定予測の次数は 0-4 (実装バグ)"
    );
    debug_assert!(
        samples.iter().all(|&s| {
            let limit = 1i32 << (BEST_ORDER_I32_MAX_BITS - 1);
            (-limit..limit).contains(&s)
        }),
        "best_order_i32 のサンプルは {} bit に収まる (実装バグ)",
        BEST_ORDER_I32_MAX_BITS
    );
    if samples.len() <= 4 {
        // 評価対象のサンプルがない。全次数同点として次数 0 を返す
        return 0;
    }
    // 総和は 33 bit 以内の絶対値が 65535 個でも u64 に収まる
    let mut total0 = 0u64;
    let mut total1 = 0u64;
    let mut total2 = 0u64;
    let mut total3 = 0u64;
    let mut total4 = 0u64;
    for window in samples.windows(5) {
        // 窓内の階差カスケード (次数 k の残差は次数 k-1 の残差の階差)
        let d1a = window[4] - window[3];
        let d1b = window[3] - window[2];
        let d1c = window[2] - window[1];
        let d1d = window[1] - window[0];
        let d2a = d1a - d1b;
        let d2b = d1b - d1c;
        let d2c = d1c - d1d;
        let d3a = d2a - d2b;
        let d3b = d2b - d2c;
        let d4a = d3a - d3b;
        total0 += u64::from(window[4].unsigned_abs());
        total1 += u64::from(d1a.unsigned_abs());
        total2 += u64::from(d2a.unsigned_abs());
        total3 += u64::from(d3a.unsigned_abs());
        total4 += u64::from(d4a.unsigned_abs());
    }
    let totals = [total0, total1, total2, total3, total4];
    // 同点では低い次数 (warm-up が少なくヘッダーが単純な方) を選ぶ
    let mut best = 0;
    for (order, &total) in totals.iter().enumerate().take(max_order + 1).skip(1) {
        if total < totals[best] {
            best = order;
        }
    }
    best
}

/// 範囲外の復元サンプルを不正データとして報告する
///
/// ホットループから呼ばれるが実行は稀なので、インライン展開せず
/// ループ本体のレジスタ圧を抑える。
#[cold]
#[inline(never)]
fn out_of_range(value: i64) -> ParseError {
    ParseError::Invalid(DecodeError::InvalidData(format!(
        "decoded sample {} exceeds the subframe bit depth range (RFC 9639 Section 5)",
        value
    )))
}

/// 固定予測の残差からサンプルを復元する (デコード)
///
/// `samples` には warm-up サンプル `order` 個に続いて残差が入っている状態で
/// 呼ぶこと。残差部分がサンプル値に置き換わる。
///
/// サンプルごとに次数の分岐が入らないよう、次数ごとの専用ループで復元する。
/// 各次数の予測値は過去サンプルの二項係数による線形結合
/// (RFC 9639 Appendix A.3 Table 26)。
///
/// 復元した各サンプルが `[low, high]` (サブフレームのビット深度の範囲) に
/// 収まることを検証する。全サンプルが範囲内であれば予測計算は 37 bit に
/// 収まり、オーバーフローしない (RFC 9639 Appendix A.3)。
pub(crate) fn restore_samples(
    samples: &mut [i64],
    order: usize,
    low: i64,
    high: i64,
) -> Result<(), ParseError> {
    debug_assert!(order <= MAX_FIXED_ORDER, "固定予測の次数は 0-4 (実装バグ)");
    debug_assert!(samples.len() >= order, "サンプル数が次数未満 (実装バグ)");
    // 直前のサンプルはレジスタ上に持ち回り、復元済みサンプルの再ロードを
    // なくす (p1 が直前、p2 がその前、の順)
    match order {
        0 => {
            // 予測値 0: 残差がそのままサンプル値になる。範囲だけ検証する
            for &value in samples.iter() {
                if value < low || value > high {
                    return Err(out_of_range(value));
                }
            }
        }
        1 => {
            let mut p1 = samples[0];
            for slot in samples[1..].iter_mut() {
                let value = *slot + p1;
                if value < low || value > high {
                    return Err(out_of_range(value));
                }
                *slot = value;
                p1 = value;
            }
        }
        2 => {
            let mut p2 = samples[0];
            let mut p1 = samples[1];
            for slot in samples[2..].iter_mut() {
                let value = *slot + 2 * p1 - p2;
                if value < low || value > high {
                    return Err(out_of_range(value));
                }
                *slot = value;
                p2 = p1;
                p1 = value;
            }
        }
        3 => {
            let mut p3 = samples[0];
            let mut p2 = samples[1];
            let mut p1 = samples[2];
            for slot in samples[3..].iter_mut() {
                let value = *slot + 3 * p1 - 3 * p2 + p3;
                if value < low || value > high {
                    return Err(out_of_range(value));
                }
                *slot = value;
                p3 = p2;
                p2 = p1;
                p1 = value;
            }
        }
        _ => {
            let mut p4 = samples[0];
            let mut p3 = samples[1];
            let mut p2 = samples[2];
            let mut p1 = samples[3];
            for slot in samples[4..].iter_mut() {
                let value = *slot + 4 * p1 - 6 * p2 + 4 * p3 - p4;
                if value < low || value > high {
                    return Err(out_of_range(value));
                }
                *slot = value;
                p4 = p3;
                p3 = p2;
                p2 = p1;
                p1 = value;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16 bit サブフレームの範囲
    const LOW16: i64 = -(1 << 15);
    const HIGH16: i64 = (1 << 15) - 1;

    /// RFC 9639 Appendix D.2 Table 40: 次数 1 の固定予測で
    /// warm-up 4302 + 残差からサンプルを復元する
    #[test]
    fn restore_rfc9639_appendix_d2_subframe1() {
        let mut samples: Vec<i64> = alloc::vec![
            4302, // warm-up
            3194, -1297, 1228, -943, 952, -696, 768, -524, 599, -401, -13172, -316, 274, -267, 134,
        ];
        restore_samples(&mut samples, 1, LOW16, HIGH16).expect("サンプル復元に成功するはず");
        assert_eq!(
            samples,
            [
                4302, 7496, 6199, 7427, 6484, 7436, 6740, 7508, 6984, 7583, 7182, -5990, -6306,
                -6032, -6299, -6165
            ]
        );
    }

    #[test]
    fn residual_restore_roundtrip_all_orders() {
        // 多項式 + ノイズ的な信号でラウンドトリップを確認する
        let signal: Vec<i64> = (0..64)
            .map(|i: i64| i * i * 3 - i * 7 + ((i * 31) % 11) - 5)
            .collect();
        for order in 0..=MAX_FIXED_ORDER {
            let mut residual = Vec::new();
            compute_residual(&signal, order, &mut residual);
            assert_eq!(residual.len(), signal.len() - order);
            let mut restored = signal[..order].to_vec();
            restored.extend_from_slice(&residual);
            restore_samples(&mut restored, order, LOW16, HIGH16)
                .expect("サンプル復元に成功するはず");
            assert_eq!(restored, signal, "次数 {} でラウンドトリップ失敗", order);
        }
    }

    /// 復元サンプルがビット深度の範囲を超えたら不正データとして拒否する。
    /// 壊れたストリームで復元値が逐次増大して整数オーバーフローするのを防ぐ
    #[test]
    fn restore_rejects_out_of_range_sample() {
        // 8 bit 範囲 (-128..=127) で warm-up 127 + 残差 1 は 128 になり範囲外
        let mut samples: Vec<i64> = alloc::vec![127, 1];
        assert!(restore_samples(&mut samples, 1, -128, 127).is_err());
    }

    /// カスケード方式の次数選択が二項係数による直接計算と一致する
    #[test]
    fn best_order_matches_direct_reference() {
        // 二項係数の線形結合 (RFC 9639 Appendix A.3 Table 26) による
        // 直接計算のリファレンス実装 (検証用)
        fn best_order_direct(samples: &[i64], max_order: usize) -> usize {
            if samples.len() <= 4 {
                return 0;
            }
            let mut totals = [0u64; MAX_FIXED_ORDER + 1];
            for i in 4..samples.len() {
                let (s0, s1, s2, s3, s4) = (
                    samples[i],
                    samples[i - 1],
                    samples[i - 2],
                    samples[i - 3],
                    samples[i - 4],
                );
                totals[0] += s0.unsigned_abs();
                totals[1] += (s0 - s1).unsigned_abs();
                totals[2] += (s0 - 2 * s1 + s2).unsigned_abs();
                totals[3] += (s0 - 3 * s1 + 3 * s2 - s3).unsigned_abs();
                totals[4] += (s0 - 4 * s1 + 6 * s2 - 4 * s3 + s4).unsigned_abs();
            }
            let mut best = 0;
            for (order, &total) in totals.iter().enumerate().take(max_order + 1).skip(1) {
                if total < totals[best] {
                    best = order;
                }
            }
            best
        }

        // 定数・直線・二次・ノイズ混在など傾向の違う決定的な信号で照合する
        let mut state = 0xACE1u64;
        for len in [5usize, 6, 17, 64, 255] {
            for variant in 0..4 {
                let signal: Vec<i64> = (0..len as i64)
                    .map(|i| {
                        state = state
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        let noise = ((state >> 56) as i64) - 128;
                        match variant {
                            0 => 42,
                            1 => 3 * i - 100 + (noise >> 6),
                            2 => i * i - 30 * i + noise,
                            _ => noise * 257,
                        }
                    })
                    .collect();
                for max_order in 0..=MAX_FIXED_ORDER {
                    assert_eq!(
                        best_order(&signal, max_order),
                        best_order_direct(&signal, max_order),
                        "長さ {} variant {} max_order {} で不一致",
                        len,
                        variant,
                        max_order
                    );
                }
            }
        }
    }

    #[test]
    fn order2_predicts_linear_signal_exactly() {
        // 直線信号は次数 2 で残差が全て 0 になる
        let signal: Vec<i64> = (0..32).map(|i| 100 + 7 * i).collect();
        let mut residual = Vec::new();
        compute_residual(&signal, 2, &mut residual);
        assert!(residual.iter().all(|&r| r == 0));
    }

    #[test]
    fn order0_residual_equals_signal() {
        let signal: Vec<i64> = alloc::vec![5, -3, 8, 0, -12];
        let mut residual = Vec::new();
        compute_residual(&signal, 0, &mut residual);
        assert_eq!(residual, signal);
    }
}
