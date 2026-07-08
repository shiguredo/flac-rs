//! 線形予測 (LPC) (RFC 9639 Section 9.2.6)
//!
//! デコードは整数演算のみで行い、ロスレス性を保証する (RFC 9639 Appendix A)。
//! エンコード時の予測器の推定 (自己相関 + Levinson-Durbin 法) には浮動小数点を
//! 使うが、推定した係数を量子化した後の残差計算は整数演算のみで行うため、
//! ロスレス性には影響しない。
//!
//! 壊れたストリームでは復元サンプルが逐次的に増大して整数オーバーフローし得る
//! ため、デコード時は復元したサンプルごとにビット深度の範囲を検証し、範囲外を
//! 即座に不正データとして拒否する (RFC 9639 Section 5)。

use alloc::format;
use alloc::vec::Vec;

use crate::error::{DecodeError, ParseError};

/// LPC の最大次数 (RFC 9639 Section 9.2.6)
pub(crate) const MAX_LPC_ORDER: usize = 32;

/// 量子化された LPC 係数の精度の最大ビット数
///
/// フォーマット上は 15 bit まで表現できる (精度 - 1 を 4 bit で格納、0b1111 は
/// 禁止) (RFC 9639 Section 9.2.6)。
pub(crate) const MAX_COEFFICIENT_PRECISION: u32 = 15;

/// 量子化シフトの最大値
///
/// シフトは signed 5 bit で格納されるが負の値は禁止のため 0-15 (RFC 9639
/// Section 9.2.6)。
pub(crate) const MAX_QUANTIZATION_SHIFT: u32 = 15;

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

/// 次数をコンパイル時定数にした復元ループ (`restore_samples` の実体)
///
/// 次数が定数になると内積が完全にアンロールされ、係数がレジスタに乗る。
/// 直前 ORDER 個のサンプルはリングバッファで持ち回り、復元済みサンプルの
/// 再ロードをなくす。外側を ORDER 段にアンロールすることで、リングの
/// 位置 (剰余) が全て定数になり、リング自体もレジスタに昇格される
/// (要素をずらすシフト操作が発生しない)。
fn restore_samples_with_order<const ORDER: usize>(
    samples: &mut [i64],
    coefficients: &[i64],
    shift: u32,
    low: i64,
    high: i64,
) -> Result<(), ParseError> {
    // 係数を古いサンプルに掛かる順に並べ替え、リングと同じ向き
    // (window[0] が最も古いサンプル) で内積を取れるようにする
    let mut reversed = [0i64; ORDER];
    for (slot, &coefficient) in reversed.iter_mut().zip(coefficients.iter().rev()) {
        *slot = coefficient;
    }
    let mut window = [0i64; ORDER];
    window.copy_from_slice(&samples[..ORDER]);
    let n = samples.len();
    let mut i = ORDER;
    // 段 j では window[j] が最も古いサンプルで、次に上書きされる位置。
    // (j + k) % ORDER が全て定数になり、リングはレジスタに乗る
    'frame: loop {
        for j in 0..ORDER {
            if i == n {
                break 'frame;
            }
            let mut prediction: i64 = 0;
            for k in 0..ORDER {
                prediction += reversed[k] * window[(j + k) % ORDER];
            }
            // 右シフトは算術シフト (負の値は負の無限大方向へ丸める)
            let value = samples[i] + (prediction >> shift);
            if value < low || value > high {
                return Err(out_of_range(value));
            }
            samples[i] = value;
            window[j] = value;
            i += 1;
        }
    }
    Ok(())
}

/// LPC の残差からサンプルを復元する (デコード)
///
/// `samples` には warm-up サンプル `coefficients.len()` 個に続いて残差が
/// 入っている状態で呼ぶこと。残差部分がサンプル値に置き換わる。
///
/// 係数はビットストリーム順、つまり `coefficients[0]` が直前のサンプルに
/// 対応する (RFC 9639 Section 9.2.6)。
///
/// 復元した各サンプルが `[low, high]` (サブフレームのビット深度の範囲) に
/// 収まることを検証する。全サンプルが範囲内であれば予測の計算は最大
/// 33 bit (サンプル) + 15 bit (係数) + 5 bit (次数 32 の総和) = 53 bit で
/// `i64` から溢れない (RFC 9639 Appendix A.3)。
pub(crate) fn restore_samples(
    samples: &mut [i64],
    coefficients: &[i64],
    shift: u32,
    low: i64,
    high: i64,
) -> Result<(), ParseError> {
    let order = coefficients.len();
    debug_assert!(
        (1..=MAX_LPC_ORDER).contains(&order),
        "LPC の次数は 1-32 (実装バグ)"
    );
    debug_assert!(samples.len() >= order, "サンプル数が次数未満 (実装バグ)");
    // 実際に使われやすい次数 (リファレンス実装 libFLAC の既定は最大 8、
    // 最高圧縮でも 12) は定数化した専用ループへ振り分け、内積をアンロール
    // させる。13 次以上は稀なので 1 つの汎用ループにまとめる
    match order {
        1 => restore_samples_with_order::<1>(samples, coefficients, shift, low, high),
        2 => restore_samples_with_order::<2>(samples, coefficients, shift, low, high),
        3 => restore_samples_with_order::<3>(samples, coefficients, shift, low, high),
        4 => restore_samples_with_order::<4>(samples, coefficients, shift, low, high),
        5 => restore_samples_with_order::<5>(samples, coefficients, shift, low, high),
        6 => restore_samples_with_order::<6>(samples, coefficients, shift, low, high),
        7 => restore_samples_with_order::<7>(samples, coefficients, shift, low, high),
        8 => restore_samples_with_order::<8>(samples, coefficients, shift, low, high),
        9 => restore_samples_with_order::<9>(samples, coefficients, shift, low, high),
        10 => restore_samples_with_order::<10>(samples, coefficients, shift, low, high),
        11 => restore_samples_with_order::<11>(samples, coefficients, shift, low, high),
        12 => restore_samples_with_order::<12>(samples, coefficients, shift, low, high),
        _ => restore_samples_generic(samples, coefficients, shift, low, high),
    }
}

/// 任意次数の復元ループ (13 次以上のフォールバック)
fn restore_samples_generic(
    samples: &mut [i64],
    coefficients: &[i64],
    shift: u32,
    low: i64,
    high: i64,
) -> Result<(), ParseError> {
    let order = coefficients.len();
    // 係数を古いサンプルに掛かる順に並べ替え、直前 order 個のサンプル窓を
    // 前向きの連続アクセスで走査する (自動ベクトル化しやすい形)
    let mut reversed: [i64; MAX_LPC_ORDER] = [0; MAX_LPC_ORDER];
    for (slot, &coefficient) in reversed[..order].iter_mut().zip(coefficients.iter().rev()) {
        *slot = coefficient;
    }
    let reversed = &reversed[..order];
    for i in order..samples.len() {
        let mut prediction: i64 = 0;
        for (&coefficient, &sample) in reversed.iter().zip(&samples[i - order..i]) {
            prediction += coefficient * sample;
        }
        // 右シフトは算術シフト (負の値は負の無限大方向へ丸める)
        let value = samples[i] + (prediction >> shift);
        if value < low || value > high {
            return Err(out_of_range(value));
        }
        samples[i] = value;
    }
    Ok(())
}

/// 次数をコンパイル時定数にした残差計算ループ (`compute_residual` の実体)
///
/// 次数が定数になると内積が完全にアンロールされる。復元と違って残差計算は
/// 出力が入力に影響しない (サンプル列は読み取り専用) ため、ループ全体が
/// 自動ベクトル化の対象になる。
fn compute_residual_with_order<const ORDER: usize>(
    samples: &[i64],
    coefficients: &[i64],
    shift: u32,
    residual: &mut Vec<i64>,
) {
    let mut reversed = [0i64; ORDER];
    for (slot, &coefficient) in reversed.iter_mut().zip(coefficients.iter().rev()) {
        *slot = coefficient;
    }
    // windows + extend は長さが既知のイテレータになり、要素ごとの容量
    // チェックを伴わない
    residual.extend(samples.windows(ORDER + 1).map(|window| {
        let mut prediction: i64 = 0;
        for (&coefficient, &sample) in reversed.iter().zip(window.iter()) {
            prediction += coefficient * sample;
        }
        window[ORDER] - (prediction >> shift)
    }));
}

/// LPC の残差を計算する (エンコード)
///
/// `samples` の先頭 `coefficients.len()` 個は warm-up サンプルとして扱う。
/// 残差は `residual` をクリアして書き込む (使い回しで再確保を避ける)。
pub(crate) fn compute_residual(
    samples: &[i64],
    coefficients: &[i64],
    shift: u32,
    residual: &mut Vec<i64>,
) {
    let order = coefficients.len();
    debug_assert!(
        (1..=MAX_LPC_ORDER).contains(&order),
        "LPC の次数は 1-32 (実装バグ)"
    );
    debug_assert!(samples.len() >= order, "サンプル数が次数未満 (実装バグ)");
    residual.clear();
    residual.reserve(samples.len() - order);
    // 実際に使われやすい次数 (リファレンス実装 libFLAC の既定は最大 8、
    // 最高圧縮でも 12) は定数化した専用ループへ振り分ける
    match order {
        1 => compute_residual_with_order::<1>(samples, coefficients, shift, residual),
        2 => compute_residual_with_order::<2>(samples, coefficients, shift, residual),
        3 => compute_residual_with_order::<3>(samples, coefficients, shift, residual),
        4 => compute_residual_with_order::<4>(samples, coefficients, shift, residual),
        5 => compute_residual_with_order::<5>(samples, coefficients, shift, residual),
        6 => compute_residual_with_order::<6>(samples, coefficients, shift, residual),
        7 => compute_residual_with_order::<7>(samples, coefficients, shift, residual),
        8 => compute_residual_with_order::<8>(samples, coefficients, shift, residual),
        9 => compute_residual_with_order::<9>(samples, coefficients, shift, residual),
        10 => compute_residual_with_order::<10>(samples, coefficients, shift, residual),
        11 => compute_residual_with_order::<11>(samples, coefficients, shift, residual),
        12 => compute_residual_with_order::<12>(samples, coefficients, shift, residual),
        _ => compute_residual_generic(samples, coefficients, shift, residual),
    }
}

/// 次数をコンパイル時定数にした残差計算ループ (i32 格納版、`compute_residual_i32` の実体)
///
/// サンプルと係数を i32 で「格納」し、積和は `i64::from` で widening してから行う。
/// 演算列は i64 版 (`compute_residual_with_order`) と完全に同一なので、結果も
/// ビット単位で一致する (格納幅を狭めただけで算術は i64 のまま)。
///
/// i64 格納では 64 bit 整数乗算がベクトル命令に存在せず (AArch64 NEON / AVX2
/// とも) スカラー乗算が上限だが、i32×i32→i64 の widening 乗算は AArch64 では
/// smull/smlal、x86_64 (AVX2) では vpmuldq に乗る。ループは「出力 4 点ブロック +
/// 係数スプラット」の形 (libFLAC の NEON 実装と同じデータフロー) にする。
/// スカラー 4 本のアキュムレータが SLP ベクトル化で v2i64 x2 になり、連続
/// ロードだけで済む。`windows().map()` を `extend` する形は隣接ウィンドウの
/// ペア化にシャッフルが挟まり、ベクトル化されても実効が出ない (実測)。
fn compute_residual_i32_with_order<const ORDER: usize>(
    samples: &[i32],
    coefficients: &[i64],
    shift: u32,
    residual: &mut Vec<i64>,
) {
    // 量子化済みの係数は精度 15 bit 以内 (RFC 9639 Section 9.2.6) なので
    // i32 に必ず収まる
    let mut reversed = [0i32; ORDER];
    for (slot, &coefficient) in reversed.iter_mut().zip(coefficients.iter().rev()) {
        debug_assert!(
            i32::try_from(coefficient).is_ok(),
            "量子化済みの LPC 係数は i32 に収まる (実装バグ)"
        );
        *slot = coefficient as i32;
    }
    let n = samples.len() - ORDER;
    // 出力 4 点ブロックの直接書き込みのため長さを先に確定する (`extend` に
    // 書き足す形は容量チェックがブロックのベクトル化を壊す。実測)。
    // バッファは呼び出し側が使い回すため、確保は初回のみ
    residual.resize(n, 0);
    let out = &mut residual[..];
    let mut i = 0;
    // 出力 4 点ブロック: 係数 c_k を 4 レーンにスプラットし、サンプル窓を
    // 1 ずつずらした連続ロードで積和する
    while i + 4 <= n {
        let mut acc0: i64 = 0;
        let mut acc1: i64 = 0;
        let mut acc2: i64 = 0;
        let mut acc3: i64 = 0;
        for (k, &coefficient) in reversed.iter().enumerate() {
            let c = i64::from(coefficient);
            let window = &samples[i + k..i + k + 4];
            acc0 += c * i64::from(window[0]);
            acc1 += c * i64::from(window[1]);
            acc2 += c * i64::from(window[2]);
            acc3 += c * i64::from(window[3]);
        }
        let current = &samples[i + ORDER..i + ORDER + 4];
        out[i] = i64::from(current[0]) - (acc0 >> shift);
        out[i + 1] = i64::from(current[1]) - (acc1 >> shift);
        out[i + 2] = i64::from(current[2]) - (acc2 >> shift);
        out[i + 3] = i64::from(current[3]) - (acc3 >> shift);
        i += 4;
    }
    // 端数 (最大 3 点) はスカラーで処理する
    while i < n {
        let mut prediction: i64 = 0;
        for (k, &coefficient) in reversed.iter().enumerate() {
            prediction += i64::from(coefficient) * i64::from(samples[i + k]);
        }
        out[i] = i64::from(samples[i + ORDER]) - (prediction >> shift);
        i += 1;
    }
}

/// LPC の残差を計算する (エンコード、i32 格納版)
///
/// `compute_residual` と同じ計算を、i32 に収まるサンプル列 (ビット深度 32 以下)
/// に対して widening 積和で行う。結果は `compute_residual` とビット単位で一致
/// する。33 bit (32 bit 音源のサイドチャンネル) は表現できないため、その場合は
/// 呼び出し側が i64 版を使うこと。
pub(crate) fn compute_residual_i32(
    samples: &[i32],
    coefficients: &[i64],
    shift: u32,
    residual: &mut Vec<i64>,
) {
    let order = coefficients.len();
    debug_assert!(
        (1..=MAX_LPC_ORDER).contains(&order),
        "LPC の次数は 1-32 (実装バグ)"
    );
    debug_assert!(samples.len() >= order, "サンプル数が次数未満 (実装バグ)");
    residual.clear();
    match order {
        1 => compute_residual_i32_with_order::<1>(samples, coefficients, shift, residual),
        2 => compute_residual_i32_with_order::<2>(samples, coefficients, shift, residual),
        3 => compute_residual_i32_with_order::<3>(samples, coefficients, shift, residual),
        4 => compute_residual_i32_with_order::<4>(samples, coefficients, shift, residual),
        5 => compute_residual_i32_with_order::<5>(samples, coefficients, shift, residual),
        6 => compute_residual_i32_with_order::<6>(samples, coefficients, shift, residual),
        7 => compute_residual_i32_with_order::<7>(samples, coefficients, shift, residual),
        8 => compute_residual_i32_with_order::<8>(samples, coefficients, shift, residual),
        9 => compute_residual_i32_with_order::<9>(samples, coefficients, shift, residual),
        10 => compute_residual_i32_with_order::<10>(samples, coefficients, shift, residual),
        11 => compute_residual_i32_with_order::<11>(samples, coefficients, shift, residual),
        12 => compute_residual_i32_with_order::<12>(samples, coefficients, shift, residual),
        _ => compute_residual_i32_generic(samples, coefficients, shift, residual),
    }
}

/// 任意次数の残差計算ループ (i32 格納版、13 次以上のフォールバック)
///
/// `compute_residual_i32_with_order` と同じ「出力 4 点ブロック + 係数スプラット」
/// の形。次数が実行時値なので係数ループは回るが、内側の 4 レーン積和は同様に
/// widening 乗算へベクトル化される。
fn compute_residual_i32_generic(
    samples: &[i32],
    coefficients: &[i64],
    shift: u32,
    residual: &mut Vec<i64>,
) {
    let order = coefficients.len();
    // 係数を古いサンプルに掛かる順に並べ替え、直前 order 個のサンプル窓を
    // 前向きの連続アクセスで走査する
    let mut reversed: [i32; MAX_LPC_ORDER] = [0; MAX_LPC_ORDER];
    for (slot, &coefficient) in reversed[..order].iter_mut().zip(coefficients.iter().rev()) {
        debug_assert!(
            i32::try_from(coefficient).is_ok(),
            "量子化済みの LPC 係数は i32 に収まる (実装バグ)"
        );
        *slot = coefficient as i32;
    }
    let reversed = &reversed[..order];
    let n = samples.len() - order;
    // 出力 4 点ブロックの直接書き込みのため長さを先に確定する
    residual.resize(n, 0);
    let out = &mut residual[..];
    let mut i = 0;
    while i + 4 <= n {
        let mut acc0: i64 = 0;
        let mut acc1: i64 = 0;
        let mut acc2: i64 = 0;
        let mut acc3: i64 = 0;
        for (k, &coefficient) in reversed.iter().enumerate() {
            let c = i64::from(coefficient);
            let window = &samples[i + k..i + k + 4];
            acc0 += c * i64::from(window[0]);
            acc1 += c * i64::from(window[1]);
            acc2 += c * i64::from(window[2]);
            acc3 += c * i64::from(window[3]);
        }
        let current = &samples[i + order..i + order + 4];
        out[i] = i64::from(current[0]) - (acc0 >> shift);
        out[i + 1] = i64::from(current[1]) - (acc1 >> shift);
        out[i + 2] = i64::from(current[2]) - (acc2 >> shift);
        out[i + 3] = i64::from(current[3]) - (acc3 >> shift);
        i += 4;
    }
    // 端数 (最大 3 点) はスカラーで処理する
    while i < n {
        let mut prediction: i64 = 0;
        for (k, &coefficient) in reversed.iter().enumerate() {
            prediction += i64::from(coefficient) * i64::from(samples[i + k]);
        }
        out[i] = i64::from(samples[i + order]) - (prediction >> shift);
        i += 1;
    }
}

/// 任意次数の残差計算ループ (13 次以上のフォールバック)
fn compute_residual_generic(
    samples: &[i64],
    coefficients: &[i64],
    shift: u32,
    residual: &mut Vec<i64>,
) {
    let order = coefficients.len();
    // 係数を古いサンプルに掛かる順に並べ替え、直前 order 個のサンプル窓を
    // 前向きの連続アクセスで走査する (自動ベクトル化しやすい形)
    let mut reversed: [i64; MAX_LPC_ORDER] = [0; MAX_LPC_ORDER];
    for (slot, &coefficient) in reversed[..order].iter_mut().zip(coefficients.iter().rev()) {
        *slot = coefficient;
    }
    let reversed = &reversed[..order];
    for i in order..samples.len() {
        let mut prediction: i64 = 0;
        for (&coefficient, &sample) in reversed.iter().zip(&samples[i - order..i]) {
            prediction += coefficient * sample;
        }
        residual.push(samples[i] - (prediction >> shift));
    }
}

/// log2 の近似 (標準 `f64::log2` より高速で、次数選択のヒューリスティックには十分な精度)
///
/// f64 のビット表現から指数部を取り出し、仮数部で線形補間する。
/// log2(1+m) ≈ m の近似を使い、誤差は 0.09 以下。
/// 正の有限値に対してのみ使うこと。
fn approx_log2(x: f64) -> f64 {
    debug_assert!(x > 0.0, "approx_log2 は正の値のみ (実装バグ)");
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7FF) as i64 - 1023;
    let mantissa = (bits & 0xF_FFFF_FFFF_FFFF) as f64 / (1u64 << 52) as f64;
    // 指数部と仮数部から近似: log2(2^e * (1 + m)) = e + log2(1 + m) ≈ e + m
    exponent as f64 + mantissa
}

/// Welch 窓を適用したサンプル列を `windowed` に書き出す (エンコードの解析用)
///
/// Welch 窓 w(n) = 1 - ((n - (N-1)/2) / ((N-1)/2))^2 は放物線状の窓関数で、
/// 三角関数が不要なため no_std でも計算できる。矩形窓よりスペクトル漏れが
/// 少なく、LPC 係数の推定品質が上がる。
///
/// 出力バッファは使い回す (計画のたびに確保すると malloc がエンコード時間の
/// 1 割を占める。実測)。
fn apply_welch_window(samples: &[i64], windowed: &mut Vec<f64>) {
    windowed.clear();
    let n = samples.len();
    if n <= 1 {
        windowed.extend(samples.iter().map(|&s| s as f64));
        return;
    }
    let half = (n - 1) as f64 / 2.0;
    // サンプルごとの除算はスループットが低いため、逆数の乗算に置き換える。
    // 丸めは除算と厳密には一致しないが、窓は係数推定のヒューリスティック
    // であり、係数はこのあと量子化されるためロスレス性には影響しない
    let inv_half = 1.0 / half;
    windowed.extend(samples.iter().enumerate().map(|(i, &s)| {
        let x = (i as f64 - half) * inv_half;
        (s as f64) * (1.0 - x * x)
    }));
}

/// ラグ `base_lag` から `LAGS` 本の自己相関を 1 回の走査で計算する
///
/// `windowed[i]` のロードを `LAGS` 本のラグで共有し、走査回数を約
/// `1 / LAGS` に減らす。f64 の加算は結合法則が成り立たず逐次和は直列の
/// 加算依存になるため、ラグごとに 4 本の独立アキュムレータへ規則的に
/// 足し込んで依存を断つ (この形は f64x2 のベクトルアキュムレータに落ち、
/// 水平和がループ内に現れない)。加算順序が変わるので丸め誤差は逐次和と
/// 一致しないが、係数はこのあと量子化され、残差計算は整数演算のみなので
/// ロスレス性には影響しない。
///
/// 積和の f32 化 (f32x4 で 2 倍幅) は試したが採用しない。単純な f32 積和は
/// 蓄積誤差 (相対 1e-7) が高予測ゲイン信号の係数品質を落とし圧縮率が悪化
/// する (tonal で +3.9%。実測)。64 サンプルごとの f64 2 段和なら精度は
/// 保てるが境界処理が利得を食い、f64 直接比 5% しか残らない (実測)。
fn accumulate_lag_group<const LAGS: usize>(windowed: &[f64], base_lag: usize) -> [f64; LAGS] {
    let n = windowed.len();
    // 全ラグの積が定義される最初の位置。i - base_lag - k >= 0 が
    // k = 0..LAGS で成り立つ
    let start = base_lag + LAGS - 1;
    let mut result = [0.0f64; LAGS];
    if start >= n {
        // 入力がラグより短い端ケース: 定義される項だけを直接足す
        for (k, slot) in result.iter_mut().enumerate() {
            let lag = base_lag + k;
            for i in lag..n {
                *slot += windowed[i] * windowed[i - lag];
            }
        }
        return result;
    }
    let mut acc = [[0.0f64; 4]; LAGS];
    let body = &windowed[start..];
    let chunk_count = body.len() / 4;
    for (j, chunk) in body.chunks_exact(4).enumerate() {
        let i = start + j * 4;
        for k in 0..LAGS {
            // ラグ k の相手側 4 要素 (連続)
            let y = &windowed[i - base_lag - k..][..4];
            acc[k][0] += chunk[0] * y[0];
            acc[k][1] += chunk[1] * y[1];
            acc[k][2] += chunk[2] * y[2];
            acc[k][3] += chunk[3] * y[3];
        }
    }
    let remainder_start = start + chunk_count * 4;
    for (k, slot) in result.iter_mut().enumerate() {
        let mut sum = (acc[k][0] + acc[k][1]) + (acc[k][2] + acc[k][3]);
        // 4 の倍数からはみ出た末尾 (0-3 要素)
        for i in remainder_start..n {
            sum += windowed[i] * windowed[i - base_lag - k];
        }
        // メインループが i = start から始まるため、大きいラグ (k > 0) ほど
        // 先頭側の項 (i = base_lag + k .. start) が欠けている。ここで補う
        for i in (base_lag + k)..start {
            sum += windowed[i] * windowed[i - base_lag - k];
        }
        *slot = sum;
    }
    result
}

/// 自己相関を計算する (エンコードの解析用)
///
/// ラグごとに独立へ走査すると `max_lag + 1` 回のパスになるため、
/// 4 本ずつまとめて走査してロードを共有する。
fn compute_autocorrelation(windowed: &[f64], max_lag: usize) -> Vec<f64> {
    let mut autocorrelation = alloc::vec![0.0f64; max_lag + 1];
    let mut lag = 0;
    while lag <= max_lag {
        // 残りのラグ数に応じたグループで処理する。既定の最大次数 8
        // (ラグ 9 本) までは 1 回の走査で全ラグをまとめられるようにする
        let remaining = max_lag - lag + 1;
        let group = match remaining {
            1 => {
                let acc = accumulate_lag_group::<1>(windowed, lag);
                autocorrelation[lag] = acc[0];
                1
            }
            2 => {
                let acc = accumulate_lag_group::<2>(windowed, lag);
                autocorrelation[lag..lag + 2].copy_from_slice(&acc);
                2
            }
            3 => {
                let acc = accumulate_lag_group::<3>(windowed, lag);
                autocorrelation[lag..lag + 3].copy_from_slice(&acc);
                3
            }
            4 => {
                let acc = accumulate_lag_group::<4>(windowed, lag);
                autocorrelation[lag..lag + 4].copy_from_slice(&acc);
                4
            }
            5 => {
                let acc = accumulate_lag_group::<5>(windowed, lag);
                autocorrelation[lag..lag + 5].copy_from_slice(&acc);
                5
            }
            6 => {
                let acc = accumulate_lag_group::<6>(windowed, lag);
                autocorrelation[lag..lag + 6].copy_from_slice(&acc);
                6
            }
            7 => {
                let acc = accumulate_lag_group::<7>(windowed, lag);
                autocorrelation[lag..lag + 7].copy_from_slice(&acc);
                7
            }
            8 => {
                let acc = accumulate_lag_group::<8>(windowed, lag);
                autocorrelation[lag..lag + 8].copy_from_slice(&acc);
                8
            }
            9 => {
                let acc = accumulate_lag_group::<9>(windowed, lag);
                autocorrelation[lag..lag + 9].copy_from_slice(&acc);
                9
            }
            // 10 本以上残っている間は 8 本ずつ処理する
            _ => {
                let acc = accumulate_lag_group::<8>(windowed, lag);
                autocorrelation[lag..lag + 8].copy_from_slice(&acc);
                8
            }
        };
        lag += group;
    }
    autocorrelation
}

/// Levinson-Durbin 法で LPC 係数を求める (エンコードの解析用)
///
/// 次数 1 から `max_order` までの各次数の係数と予測誤差を返す。
/// 返される係数は「直前のサンプルに掛かる係数が先頭」の順 (ビットストリーム順)。
fn levinson_durbin(autocorrelation: &[f64], max_order: usize) -> Vec<(Vec<f64>, f64)> {
    let mut results = Vec::new();
    let mut error = autocorrelation[0];
    let mut coefficients: Vec<f64> = Vec::new();

    for order in 1..=max_order {
        // 反射係数を計算する
        let mut acc = autocorrelation[order];
        for (j, &c) in coefficients.iter().enumerate() {
            acc -= c * autocorrelation[order - 1 - j];
        }
        let reflection = if error != 0.0 { acc / error } else { 0.0 };

        // 係数を更新する
        let mut next = Vec::new();
        for j in 0..order - 1 {
            next.push(coefficients[j] - reflection * coefficients[order - 2 - j]);
        }
        next.push(reflection);
        coefficients = next;

        error *= 1.0 - reflection * reflection;
        results.push((coefficients.clone(), error));
    }
    results
}

/// 浮動小数点の LPC 係数を整数係数と右シフトに量子化する (エンコードの解析用)
///
/// 量子化誤差のフィードバック付きで丸める。返り値は (係数列, シフト)。
/// 係数は `precision` bit の signed two's complement に収まる。
fn quantize_coefficients(coefficients: &[f64], precision: u32) -> (Vec<i64>, u32) {
    debug_assert!(
        (2..=MAX_COEFFICIENT_PRECISION).contains(&precision),
        "係数精度は 2-15 bit (実装バグ)"
    );
    // 最大絶対値から必要なシフト量を求める
    let max_magnitude = coefficients
        .iter()
        .fold(0.0f64, |acc, &c| if c.abs() > acc { c.abs() } else { acc });
    if max_magnitude == 0.0 {
        // 全係数 0 (無音など)。シフト 0 で全て 0 の係数を返す
        return (coefficients.iter().map(|_| 0).collect(), 0);
    }

    // max_magnitude * 2^shift が precision bit の signed 最大値に収まる最大の
    // shift を探す (シフトは 0-15 に制限される)
    let limit = (1i64 << (precision - 1)) - 1;
    let mut shift = 0u32;
    while shift < MAX_QUANTIZATION_SHIFT {
        let scaled = max_magnitude * f64::from(1u32 << (shift + 1));
        if scaled > limit as f64 {
            break;
        }
        shift += 1;
    }

    // 誤差フィードバック付きで丸める
    let scale = f64::from(1u32 << shift);
    let mut quantized = Vec::new();
    let mut carry = 0.0f64;
    for &coefficient in coefficients {
        let ideal = coefficient * scale + carry;
        // 四捨五入 (round half away from zero)。f64 -> i64 の as 変換は
        // 飽和するため未定義動作にはならない
        let rounded = if ideal >= 0.0 {
            (ideal + 0.5) as i64
        } else {
            (ideal - 0.5) as i64
        };
        let clamped = rounded.clamp(-(1i64 << (precision - 1)), limit);
        carry = ideal - clamped as f64;
        quantized.push(clamped);
    }
    (quantized, shift)
}

/// LPC 解析の結果 (エンコード用)
#[derive(Debug, Clone)]
pub(crate) struct LpcAnalysis {
    /// 量子化済み係数 (ビットストリーム順)
    pub(crate) coefficients: Vec<i64>,
    /// 係数の精度 (ビット数)
    pub(crate) precision: u32,
    /// 予測の右シフト量
    pub(crate) shift: u32,
}

/// サンプル列から LPC 予測器を推定する (エンコードの解析用)
///
/// 次数 1 から `max_order` を試し、推定符号量が最小の次数を選ぶ。
/// 信号が定数などで LPC が意味を持たない場合は `None` を返す。
///
/// `windowed` は窓適用済みサンプルの作業バッファで、呼び出し間で使い回す
/// (中身は毎回上書きされる)。
pub(crate) fn analyze(
    samples: &[i64],
    max_order: usize,
    precision: u32,
    windowed: &mut Vec<f64>,
) -> Option<LpcAnalysis> {
    let max_order = max_order.min(MAX_LPC_ORDER).min(samples.len() / 2);
    if max_order == 0 {
        return None;
    }

    apply_welch_window(samples, windowed);
    let autocorrelation = compute_autocorrelation(windowed, max_order);
    if autocorrelation[0] == 0.0 {
        // 全サンプルが 0 (窓適用後)。LPC は不要
        return None;
    }

    let candidates = levinson_durbin(&autocorrelation, max_order);

    // 予測誤差から次数ごとの期待符号量を見積もり、最小の次数を選ぶ。
    // 期待符号量 = 残差のビット数 (誤差の対数に比例) * サンプル数 + ヘッダー
    let block_size = samples.len() as f64;
    let mut best_order = 1;
    let mut best_estimate = f64::INFINITY;
    for (index, (_, error)) in candidates.iter().enumerate() {
        let order = index + 1;
        // 予測誤差 error は残差の分散に相当する。ラプラス分布を仮定した
        // 期待 Rice 符号長の近似として 0.5 * log2(分散) を使う
        let error_per_sample = (error / block_size).max(1e-9);
        let bits_per_residual = (0.5 * approx_log2(error_per_sample)).max(0.0) + 1.0;
        let estimate = bits_per_residual * (block_size - order as f64)
            + (order as f64) * f64::from(precision + 17);
        if estimate < best_estimate {
            best_estimate = estimate;
            best_order = order;
        }
    }

    let (coefficients, _) = &candidates[best_order - 1];
    let (quantized, shift) = quantize_coefficients(coefficients, precision);
    // 量子化の結果、全係数が 0 になったら LPC は無意味
    if quantized.iter().all(|&c| c == 0) {
        return None;
    }
    Some(LpcAnalysis {
        coefficients: quantized,
        precision,
        shift,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8 bit / 16 bit サブフレームの範囲
    const LOW8: i64 = -(1 << 7);
    const HIGH8: i64 = (1 << 7) - 1;
    const LOW16: i64 = -(1 << 15);
    const HIGH16: i64 = (1 << 15) - 1;

    /// RFC 9639 Appendix D.3: 係数 [7, -6, 2]、シフト 2 の LPC で
    /// warm-up [0, 79, 111] + 残差からサンプルを復元する
    #[test]
    fn restore_rfc9639_appendix_d3() {
        let mut samples: Vec<i64> = alloc::vec![0, 79, 111, 3, -1, -13, -10, -6, 2, 8, 8, 6];
        restore_samples(&mut samples, &[7, -6, 2], 2, LOW8, HIGH8)
            .expect("サンプル復元に成功するはず");
        assert_eq!(samples, [0, 79, 111, 78, 8, -61, -90, -68, -13, 42, 67, 53]);
    }

    /// 復元サンプルがビット深度の範囲を超えたら不正データとして拒否する。
    /// 壊れたストリームで復元値が逐次増大して整数オーバーフローするのを防ぐ
    #[test]
    fn restore_rejects_out_of_range_sample() {
        // 大きな係数で復元値が範囲を超えるケース
        let mut samples: Vec<i64> = alloc::vec![100, 100];
        assert!(restore_samples(&mut samples, &[16384], 0, LOW8, HIGH8).is_err());
    }

    #[test]
    fn negative_prediction_shifts_toward_negative_infinity() {
        // RFC 9639 Appendix D.3 Table 49: -190 >> 2 = -48 (負の無限大方向)
        assert_eq!(-190i64 >> 2, -48);
        assert_eq!(-319i64 >> 2, -80);
    }

    #[test]
    fn residual_restore_roundtrip() {
        // 減衰する正弦波っぽい整数信号
        let signal: Vec<i64> = (0..128)
            .map(|i: i64| {
                let phase = (i * 13) % 64 - 32;
                phase * (128 - i) / 4
            })
            .collect();
        let coefficients: Vec<i64> = alloc::vec![3, -2, 1];
        let shift = 1;
        let mut residual = Vec::new();
        compute_residual(&signal, &coefficients, shift, &mut residual);
        let mut restored = signal[..3].to_vec();
        restored.extend_from_slice(&residual);
        restore_samples(&mut restored, &coefficients, shift, LOW16, HIGH16)
            .expect("サンプル復元に成功するはず");
        assert_eq!(restored, signal);
    }

    #[test]
    fn analyze_finds_predictor_for_linear_signal() {
        // 線形信号は低次の LPC でほぼ完全に予測できる
        let signal: Vec<i64> = (0..256).map(|i| 3 * i - 128).collect();
        let analysis =
            analyze(&signal, 8, 14, &mut Vec::new()).expect("線形信号で LPC が見つかるはず");
        assert!((1..=8).contains(&analysis.coefficients.len()));
        assert!(analysis.shift <= MAX_QUANTIZATION_SHIFT);
        let limit = 1i64 << (analysis.precision - 1);
        assert!(
            analysis
                .coefficients
                .iter()
                .all(|&c| (-limit..limit).contains(&c))
        );
        // 残差が小さいこと (完全予測に近い)
        let mut residual = Vec::new();
        compute_residual(
            &signal,
            &analysis.coefficients,
            analysis.shift,
            &mut residual,
        );
        let max_residual = residual.iter().map(|r| r.abs()).max().unwrap_or(0);
        assert!(max_residual <= 4, "残差が大きすぎる: {}", max_residual);
    }

    #[test]
    fn analyze_returns_none_for_silence() {
        let signal: Vec<i64> = alloc::vec![0; 64];
        assert!(analyze(&signal, 8, 14, &mut Vec::new()).is_none());
    }

    #[test]
    fn quantize_respects_precision_limit() {
        let coefficients = [1.9, -1.5, 0.3, 123.0];
        let (quantized, shift) = quantize_coefficients(&coefficients, 5);
        let limit = 1i64 << 4;
        assert!(quantized.iter().all(|&c| (-limit..limit).contains(&c)));
        assert!(shift <= MAX_QUANTIZATION_SHIFT);
    }
}
