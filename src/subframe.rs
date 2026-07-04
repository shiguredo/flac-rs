//! サブフレーム (RFC 9639 Section 9.2)
//!
//! フレームにはチャンネル数分のサブフレームが順に格納される。各サブフレームは
//! ヘッダー (タイプ + wasted bits) と本体 (CONSTANT / VERBATIM / FIXED / LPC)
//! からなる。
//!
//! サンプル値は全て `i64` で扱う。サブフレームのビット深度は最大 33 bit
//! (32 bit + サイドチャンネルの 1 bit) であり、`i32` には収まらないため
//! (RFC 9639 Appendix A.2)。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::bit_reader::BitReader;
use crate::bit_writer::BitWriter;
use crate::error::{DecodeError, ParseError};
use crate::fixed;
use crate::lpc;
use crate::rice::{ResidualPlan, RiceScratch, decode_residual};

/// サブフレームをデコードする (RFC 9639 Section 9.2)
///
/// `bits_per_sample` はこのサブフレームのビット深度 (サイドチャンネルの +1 を
/// 適用済み、最大 33)。wasted bits のシフトを適用したサンプル列を `samples` に
/// 格納する (バッファはクリアして使い回す)。
pub(crate) fn decode_subframe(
    reader: &mut BitReader<'_>,
    block_size: u16,
    bits_per_sample: u32,
    samples: &mut Vec<i64>,
) -> Result<(), ParseError> {
    samples.clear();
    // サブフレームヘッダー (RFC 9639 Section 9.2.1)
    let pad = reader.read_bit()?;
    if pad {
        return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
            "subframe header must start with a zero bit (RFC 9639 Section 9.2.1)",
        ))));
    }
    let subframe_type = reader.read_u32(6)?;

    // wasted bits (RFC 9639 Section 9.2.2)
    let wasted_bits = if reader.read_bit()? {
        let k = reader.read_unary()? + 1;
        // wasted bits 適用後のビット深度は 1 以上でなければならない
        if k >= u64::from(bits_per_sample) {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "wasted bits {} leaves no bits for samples of depth {} (RFC 9639 Section 9.2.2)",
                k, bits_per_sample
            ))));
        }
        k as u32
    } else {
        0
    };
    let coded_bits = bits_per_sample - wasted_bits;
    // 復元サンプルが収まるべき範囲 (coded_bits の signed two's complement)。
    // 予測復元中の逐次検証に使い、壊れた入力による整数オーバーフローを防ぐ
    let low = -(1i64 << (coded_bits - 1));
    let high = (1i64 << (coded_bits - 1)) - 1;

    match subframe_type {
        // CONSTANT (RFC 9639 Section 9.2.3)
        0b000000 => {
            let value = reader.read_i64(coded_bits)?;
            for _ in 0..block_size {
                samples.push(value);
            }
        }
        // VERBATIM (RFC 9639 Section 9.2.4)
        0b000001 => {
            for _ in 0..block_size {
                samples.push(reader.read_i64(coded_bits)?);
            }
        }
        // FIXED (RFC 9639 Section 9.2.5)
        0b001000..=0b001100 => {
            let order = (subframe_type - 0b001000) as usize;
            if order >= usize::from(block_size) {
                return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                    "fixed predictor order {} must be less than block size {} (RFC 9639 Section 9.2.7)",
                    order, block_size
                ))));
            }
            // warm-up サンプル (RFC 9639 Section 9.2.5 Table 21)
            for _ in 0..order {
                samples.push(reader.read_i64(coded_bits)?);
            }
            decode_residual(reader, block_size, order as u32, samples)?;
            fixed::restore_samples(samples, order, low, high)?;
        }
        // LPC (RFC 9639 Section 9.2.6)
        0b100000..=0b111111 => {
            let order = (subframe_type - 31) as usize;
            if order >= usize::from(block_size) {
                return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                    "linear predictor order {} must be less than block size {} (RFC 9639 Section 9.2.7)",
                    order, block_size
                ))));
            }
            // warm-up サンプル (RFC 9639 Section 9.2.6 Table 22)
            for _ in 0..order {
                samples.push(reader.read_i64(coded_bits)?);
            }
            // 係数精度 - 1 (0b1111 は禁止) (RFC 9639 Section 9.2.6)
            let precision_bits = reader.read_u32(4)?;
            if precision_bits == 0b1111 {
                return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                    "predictor coefficient precision bits 0b1111 is forbidden (RFC 9639 Section 9.2.6)",
                ))));
            }
            let precision = precision_bits + 1;
            // 予測右シフト (負は禁止) (RFC 9639 Section 9.2.6)
            let shift = reader.read_i64(5)?;
            if shift < 0 {
                return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                    "negative prediction right shift {} is forbidden (RFC 9639 Section 9.2.6)",
                    shift
                ))));
            }
            // 係数はビットストリーム順 (直前のサンプルに掛かる係数が先頭)
            let mut coefficients = Vec::new();
            for _ in 0..order {
                coefficients.push(reader.read_i64(precision)?);
            }
            decode_residual(reader, block_size, order as u32, samples)?;
            lpc::restore_samples(samples, &coefficients, shift as u32, low, high)?;
        }
        _ => {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "subframe type {:#08b} is reserved (RFC 9639 Section 9.2.1)",
                subframe_type
            ))));
        }
    }

    // wasted bits 分を左シフトして復元する (RFC 9639 Section 9.2.2)
    if wasted_bits > 0 {
        for sample in samples.iter_mut() {
            *sample <<= wasted_bits;
        }
    }
    Ok(())
}

/// サブフレームの種類とエンコードに必要なデータ
///
/// 書き出しに必要な最小限のデータだけを所有する。予測系は warm-up
/// サンプル (最大 32 個) と残差だけでよく、サンプル列全体のコピーを
/// 持つのは VERBATIM のみ。
#[derive(Debug, Clone)]
enum SubframeKind {
    /// CONSTANT: 全サンプル同値
    Constant { value: i64 },
    /// VERBATIM: 全サンプル未符号化
    Verbatim { samples: Vec<i64> },
    /// FIXED: 固定予測 (次数は warm-up サンプル数)
    Fixed {
        warmup: Vec<i64>,
        residual: Vec<i64>,
        plan: ResidualPlan,
    },
    /// LPC: 線形予測 (次数は係数の個数)
    Lpc {
        analysis: lpc::LpcAnalysis,
        warmup: Vec<i64>,
        residual: Vec<i64>,
        plan: ResidualPlan,
    },
}

/// サブフレームのエンコード計画 (RFC 9639 Section 9.2)
///
/// エンコード方法を決定し、正確なビット数を見積もる。ステレオデコリレーションの
/// モード選択のために「計画を立ててから最良のものを書き出す」二段構えにする。
#[derive(Debug, Clone)]
pub(crate) struct SubframePlan {
    kind: SubframeKind,
    /// wasted bits の数 (RFC 9639 Section 9.2.2)
    wasted_bits: u32,
    /// wasted bits 適用後のビット深度
    coded_bits: u32,
    /// ブロックサイズ (サブフレームのサンプル数)
    block_size: u16,
    /// サブフレーム全体のビット数
    total_bits: u64,
}

/// エンコード計画のパラメータ
#[derive(Debug, Clone, Copy)]
pub(crate) struct SubframeOptions {
    /// LPC の最大次数 (0 なら LPC を使わない)
    pub(crate) max_lpc_order: usize,
    /// LPC 係数の量子化精度 (ビット数)
    pub(crate) lpc_precision: u32,
    /// Rice パーティションの最大オーダー
    pub(crate) max_partition_order: u32,
}

/// サブフレーム計画で使い回す作業バッファ
///
/// 計画のたびに確保すると malloc/free がエンコード時間の 1 割を占める
/// (実測) ため、エンコーダーが 1 個保持して全計画で再利用する。
/// 中身は計画のたびに上書きされる。
#[derive(Debug, Default)]
pub(crate) struct PlanScratch {
    /// i32 格納のサンプル列 (ビット深度 32 以下のとき使用)
    samples_i32: Vec<i32>,
    /// Rice 残差計画の作業領域
    rice: RiceScratch,
    /// 窓適用済みサンプル (LPC 解析の作業領域)
    windowed: Vec<f64>,
    /// 残差バッファの返却プール
    ///
    /// 残差 (最大でブロックサイズ個の i64) は計画が採用されると
    /// `SubframeKind` が所有するため、書き出しが終わった計画から
    /// `SubframePlan::recycle` で回収して次の計画で使い回す
    residual_pool: Vec<Vec<i64>>,
}

impl PlanScratch {
    /// プールから残差バッファを取り出す (なければ新規作成)
    fn take_residual(pool: &mut Vec<Vec<i64>>) -> Vec<i64> {
        pool.pop().unwrap_or_default()
    }
}

/// wasted bits の検出と CONSTANT 判定を 1 回の走査で行う
///
/// 返り値は (wasted bits, 全サンプル同値か)。wasted bits は全サンプルに
/// 共通する下位のゼロビット数 (RFC 9639 Section 9.2.2)。全サンプルが 0 の
/// 場合は 0 (CONSTANT で符号化されるため wasted bits は不要)。
fn scan_samples(samples: &[i64], bits_per_sample: u32) -> (u32, bool) {
    let first = samples[0];
    let mut all_or: i64 = 0;
    let mut all_same = true;
    for &sample in samples {
        all_or |= sample;
        all_same &= sample == first;
    }
    if all_or == 0 {
        return (0, all_same);
    }
    // 適用後のビット深度が 1 以上になるよう制限する
    (all_or.trailing_zeros().min(bits_per_sample - 1), all_same)
}

impl SubframePlan {
    /// サンプル列からエンコード計画を立てる
    ///
    /// `bits_per_sample` はこのサブフレームのビット深度 (サイドチャンネルの +1 を
    /// 適用済み)。CONSTANT / FIXED / LPC / VERBATIM を検討し、最小ビットの計画を
    /// 返す。残差が 32 bit に収まらない場合は VERBATIM に退避する
    /// (RFC 9639 Section 9.2.7.3)。
    pub(crate) fn new(
        samples: &[i64],
        bits_per_sample: u32,
        options: &SubframeOptions,
        scratch: &mut PlanScratch,
    ) -> SubframePlan {
        debug_assert!(
            !samples.is_empty(),
            "サブフレームは 1 サンプル以上 (実装バグ)"
        );
        let block_size = samples.len() as u16;

        // wasted bits の検出と CONSTANT 判定は 1 回の走査で同時に行う
        let (wasted_bits, all_same) = scan_samples(samples, bits_per_sample);
        let coded_bits = bits_per_sample - wasted_bits;

        // サブフレームヘッダー: 予約 1 bit + タイプ 6 bit + wasted フラグ 1 bit
        // + wasted bits の unary (k ビット) (RFC 9639 Section 9.2.1, 9.2.2)
        let header_bits = 8 + u64::from(wasted_bits);

        // CONSTANT: 全サンプルが同値ならこれが最小 (RFC 9639 Section 9.2.3)
        if all_same {
            let total_bits = header_bits + u64::from(coded_bits);
            return SubframePlan {
                kind: SubframeKind::Constant {
                    value: samples[0] >> wasted_bits,
                },
                wasted_bits,
                coded_bits,
                block_size,
                total_bits,
            };
        }

        // シフト済みのサンプル列は wasted bits があるときだけ作り、
        // なければ入力をそのまま参照する
        let shifted: Vec<i64>;
        let samples: &[i64] = if wasted_bits > 0 {
            shifted = samples.iter().map(|&s| s >> wasted_bits).collect();
            &shifted
        } else {
            samples
        };

        // VERBATIM は常に使える退避先 (RFC 9639 Section 9.2.4)
        let verbatim_bits = header_bits + u64::from(block_size) * u64::from(coded_bits);
        let mut best_kind: Option<SubframeKind> = None;
        let mut best_bits = verbatim_bits;

        // 作業バッファを個別の可変参照に分解する (samples_i32 を参照しながら
        // folded を可変借用できるようにする)
        let PlanScratch {
            samples_i32,
            rice,
            windowed,
            residual_pool,
        } = scratch;

        // サンプルが i32 に収まるなら i32 格納のサンプル列も用意する
        // (coded_bits が 33 になるのは 32 bit 音源のサイドチャンネルのみ)。
        // 固定予測と LPC の残差計算は演算を i64 のまま行うので結果はビット
        // 単位で同一だが、i32 格納は widening 演算の自動ベクトル化が効く
        let samples_i32: Option<&[i32]> = if coded_bits <= 32 {
            samples_i32.clear();
            samples_i32.extend(samples.iter().map(|&s| s as i32));
            Some(samples_i32)
        } else {
            None
        };

        // FIXED: 残差の絶対値和が最小の次数を 1 パスで選び、その次数の残差
        // だけを生成する (RFC 9639 Section 9.2.5)
        let max_fixed_order = fixed::MAX_FIXED_ORDER.min(samples.len() - 1);
        let mut residual = PlanScratch::take_residual(residual_pool);
        let order = match samples_i32 {
            Some(samples_i32) => {
                // 次数選択の i32 カスケードは階差で最大 4 bit 膨らむため、
                // ビット深度に余裕があるときだけ使う
                let order = if coded_bits <= fixed::BEST_ORDER_I32_MAX_BITS {
                    fixed::best_order_i32(samples_i32, max_fixed_order)
                } else {
                    fixed::best_order(samples, max_fixed_order)
                };
                fixed::compute_residual_i32(samples_i32, order, &mut residual);
                order
            }
            None => {
                let order = fixed::best_order(samples, max_fixed_order);
                fixed::compute_residual(samples, order, &mut residual);
                order
            }
        };
        let plan = ResidualPlan::new(
            &residual,
            block_size,
            order as u32,
            options.max_partition_order,
            rice,
        );
        // 残差が 32 bit に収まらない場合は Rice 符号で表現できない
        // (RFC 9639 Section 9.2.7.3)。判定は計画が集めた統計で行い、
        // 残差の走査を増やさない
        let mut fixed_residual = Some(residual);
        if plan.fits_32bit() {
            let bits = header_bits + u64::from(coded_bits) * order as u64 + plan.bits();
            if bits < best_bits {
                best_bits = bits;
                best_kind = Some(SubframeKind::Fixed {
                    warmup: samples[..order].to_vec(),
                    residual: fixed_residual
                        .take()
                        .expect("固定予測の残差は未使用 (実装バグ)"),
                    plan,
                });
            }
        }
        if let Some(unused) = fixed_residual {
            residual_pool.push(unused);
        }

        // LPC (RFC 9639 Section 9.2.6)
        if options.max_lpc_order > 0
            && let Some(analysis) = lpc::analyze(
                samples,
                options.max_lpc_order,
                options.lpc_precision,
                windowed,
            )
        {
            let mut residual = PlanScratch::take_residual(residual_pool);
            match samples_i32 {
                Some(samples_i32) => {
                    lpc::compute_residual_i32(
                        samples_i32,
                        &analysis.coefficients,
                        analysis.shift,
                        &mut residual,
                    );
                }
                None => {
                    lpc::compute_residual(
                        samples,
                        &analysis.coefficients,
                        analysis.shift,
                        &mut residual,
                    );
                }
            }
            let order = analysis.coefficients.len();
            let plan = ResidualPlan::new(
                &residual,
                block_size,
                order as u32,
                options.max_partition_order,
                rice,
            );
            let mut lpc_residual = Some(residual);
            if plan.fits_32bit() {
                let bits = header_bits
                    + u64::from(coded_bits) * order as u64
                    + 4
                    + 5
                    + u64::from(analysis.precision) * order as u64
                    + plan.bits();
                if bits < best_bits {
                    best_bits = bits;
                    // 直前まで最良だった固定予測の計画が負けた場合は、その
                    // 残差バッファを回収してから置き換える
                    if let Some(SubframeKind::Fixed { residual, .. }) = best_kind.take() {
                        residual_pool.push(residual);
                    }
                    best_kind = Some(SubframeKind::Lpc {
                        analysis,
                        warmup: samples[..order].to_vec(),
                        residual: lpc_residual.take().expect("LPC の残差は未使用 (実装バグ)"),
                        plan,
                    });
                }
            }
            if let Some(unused) = lpc_residual {
                residual_pool.push(unused);
            }
        }

        SubframePlan {
            kind: best_kind.unwrap_or_else(|| SubframeKind::Verbatim {
                samples: samples.to_vec(),
            }),
            wasted_bits,
            coded_bits,
            block_size,
            total_bits: best_bits,
        }
    }

    /// 書き出しが終わった計画から残差バッファを回収する
    ///
    /// 回収したバッファは次のサブフレーム計画の残差計算で再利用され、
    /// フレームごとの malloc を避けられる。
    pub(crate) fn recycle(self, scratch: &mut PlanScratch) {
        match self.kind {
            SubframeKind::Fixed { residual, .. } | SubframeKind::Lpc { residual, .. } => {
                scratch.residual_pool.push(residual);
            }
            SubframeKind::Verbatim { samples } => {
                scratch.residual_pool.push(samples);
            }
            SubframeKind::Constant { .. } => {}
        }
    }

    /// この計画のサブフレーム全体のビット数
    pub(crate) fn bits(&self) -> u64 {
        self.total_bits
    }

    /// 計画に従ってサブフレームを書き出す
    pub(crate) fn encode(&self, writer: &mut BitWriter) {
        // サブフレームヘッダー (RFC 9639 Section 9.2.1)
        writer.write_u32(0, 1);
        let type_bits: u32 = match &self.kind {
            SubframeKind::Constant { .. } => 0b000000,
            SubframeKind::Verbatim { .. } => 0b000001,
            SubframeKind::Fixed { warmup, .. } => 0b001000 + warmup.len() as u32,
            SubframeKind::Lpc { analysis, .. } => 31 + analysis.coefficients.len() as u32,
        };
        writer.write_u32(type_bits, 6);
        // wasted bits フラグと unary の k - 1 (RFC 9639 Section 9.2.2)
        if self.wasted_bits > 0 {
            writer.write_u32(1, 1);
            writer.write_unary(u64::from(self.wasted_bits) - 1);
        } else {
            writer.write_u32(0, 1);
        }

        match &self.kind {
            SubframeKind::Constant { value } => {
                writer.write_i64(*value, self.coded_bits);
            }
            SubframeKind::Verbatim { samples } => {
                for &sample in samples {
                    writer.write_i64(sample, self.coded_bits);
                }
            }
            SubframeKind::Fixed {
                warmup,
                residual,
                plan,
            } => {
                // warm-up サンプル (RFC 9639 Section 9.2.5)
                for &sample in warmup {
                    writer.write_i64(sample, self.coded_bits);
                }
                plan.encode(writer, residual, self.block_size, warmup.len() as u32);
            }
            SubframeKind::Lpc {
                analysis,
                warmup,
                residual,
                plan,
            } => {
                // warm-up サンプル (RFC 9639 Section 9.2.6)
                for &sample in warmup {
                    writer.write_i64(sample, self.coded_bits);
                }
                writer.write_u32(analysis.precision - 1, 4);
                writer.write_i64(i64::from(analysis.shift), 5);
                for &coefficient in &analysis.coefficients {
                    writer.write_i64(coefficient, analysis.precision);
                }
                plan.encode(writer, residual, self.block_size, warmup.len() as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_options() -> SubframeOptions {
        SubframeOptions {
            max_lpc_order: 8,
            lpc_precision: 14,
            max_partition_order: 4,
        }
    }

    /// 計画 → エンコード → デコードのラウンドトリップ
    fn roundtrip(samples: &[i64], bits_per_sample: u32, options: &SubframeOptions) {
        let plan = SubframePlan::new(
            samples,
            bits_per_sample,
            options,
            &mut PlanScratch::default(),
        );
        let mut writer = BitWriter::new();
        plan.encode(&mut writer);
        let bytes = writer.into_bytes();
        // ビット数の見積もりが実際の書き込みと一致する
        assert_eq!(bytes.len(), (plan.bits() as usize).div_ceil(8));
        let mut reader = BitReader::new(&bytes);
        let mut decoded = Vec::new();
        decode_subframe(
            &mut reader,
            samples.len() as u16,
            bits_per_sample,
            &mut decoded,
        )
        .unwrap();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn roundtrip_constant() {
        roundtrip(&[42i64; 64], 16, &default_options());
    }

    #[test]
    fn roundtrip_constant_negative() {
        roundtrip(&[-1i64; 16], 8, &default_options());
    }

    #[test]
    fn roundtrip_silence() {
        roundtrip(&[0i64; 32], 16, &default_options());
    }

    #[test]
    fn roundtrip_linear_signal() {
        let samples: Vec<i64> = (0..128).map(|i| 3 * i - 100).collect();
        roundtrip(&samples, 16, &default_options());
    }

    #[test]
    fn roundtrip_noisy_signal() {
        // 擬似乱数的な信号 (LCG)
        let mut state = 12345u64;
        let samples: Vec<i64> = (0..256)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 40) as i64) - (1 << 23)
            })
            .collect();
        roundtrip(&samples, 25, &default_options());
    }

    #[test]
    fn roundtrip_with_wasted_bits() {
        // 全サンプルの下位 3 bit が 0 → wasted bits 3
        let samples: Vec<i64> = (0..64).map(|i| (i * 5 - 100) * 8).collect();
        roundtrip(&samples, 16, &default_options());
    }

    #[test]
    fn roundtrip_without_lpc() {
        let samples: Vec<i64> = (0..64).map(|i| i * i - 1000).collect();
        let options = SubframeOptions {
            max_lpc_order: 0,
            ..default_options()
        };
        roundtrip(&samples, 17, &options);
    }

    #[test]
    fn roundtrip_33bit_side_channel_extremes() {
        // サイドチャンネルは 33 bit になる (RFC 9639 Appendix A.2)
        let max = (1i64 << 32) - 1;
        let min = -(1i64 << 32);
        // 定数だと constant になるので変化をつける
        let samples: Vec<i64> = alloc::vec![
            max, min, max, min, 0, 1, -1, max, min, 12345, -6789, 0, max, min, 42, -42
        ];
        roundtrip(&samples, 33, &default_options());
    }

    #[test]
    fn roundtrip_single_sample() {
        // 最終フレームはブロックサイズ 1 になり得る
        roundtrip(&[12345i64], 16, &default_options());
    }

    /// RFC 9639 Appendix D.1 の第 1 サブフレーム:
    /// 0x03 0x58 0xFD (verbatim + wasted bits 2、14 bit サンプル 6397)
    #[test]
    fn decode_subframe_rfc9639_appendix_d1() {
        let data = [0x03, 0x58, 0xFD];
        let mut reader = BitReader::new(&data);
        let mut samples = Vec::new();
        decode_subframe(&mut reader, 1, 16, &mut samples).unwrap();
        // 6397 << 2 = 25588
        assert_eq!(samples, [25588]);
    }

    #[test]
    fn decode_rejects_reserved_type() {
        // タイプ 0b000010 (予約済み)
        let data = [0b00000100, 0x00, 0x00, 0x00];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            decode_subframe(&mut reader, 4, 16, &mut Vec::new()),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_nonzero_pad_bit() {
        let data = [0b10000000, 0x00, 0x00, 0x00];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            decode_subframe(&mut reader, 4, 16, &mut Vec::new()),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_excessive_wasted_bits() {
        // wasted bits 16 (unary 15 個の 0 + 1) はビット深度 16 を使い切る
        let mut writer = BitWriter::new();
        writer.write_u32(0, 1);
        writer.write_u32(0b000001, 6);
        writer.write_u32(1, 1); // wasted フラグ
        writer.write_unary(15); // k - 1 = 15 → k = 16
        writer.write_u64(0, 64);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        assert!(matches!(
            decode_subframe(&mut reader, 4, 16, &mut Vec::new()),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_forbidden_lpc_precision() {
        // LPC 次数 1、precision bits 0b1111 (禁止)
        let mut writer = BitWriter::new();
        writer.write_u32(0, 1);
        writer.write_u32(0b100000, 6); // LPC 次数 1
        writer.write_u32(0, 1);
        writer.write_i64(0, 16); // warm-up
        writer.write_u32(0b1111, 4); // 禁止された精度ビット
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        assert!(matches!(
            decode_subframe(&mut reader, 4, 16, &mut Vec::new()),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_negative_lpc_shift() {
        // LPC 次数 1、シフト -1 (禁止)
        let mut writer = BitWriter::new();
        writer.write_u32(0, 1);
        writer.write_u32(0b100000, 6);
        writer.write_u32(0, 1);
        writer.write_i64(0, 16); // warm-up
        writer.write_u32(0, 4); // 精度 1 bit
        writer.write_i64(-1, 5); // 負のシフト
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        assert!(matches!(
            decode_subframe(&mut reader, 4, 16, &mut Vec::new()),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_order_exceeding_block_size() {
        // 固定予測次数 4 はブロックサイズ 4 以上でなければならない
        let mut writer = BitWriter::new();
        writer.write_u32(0, 1);
        writer.write_u32(0b001100, 6); // fixed 次数 4
        writer.write_u32(0, 1);
        writer.write_u64(0, 64);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        assert!(matches!(
            decode_subframe(&mut reader, 4, 16, &mut Vec::new()),
            Err(ParseError::Invalid(_))
        ));
    }
}
