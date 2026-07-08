//! 残差の符号化 (RFC 9639 Section 9.2.7)
//!
//! 残差はパーティション分割された Rice 符号で格納される。各パーティションは
//! Rice パラメータ (またはエスケープコード + 固定長) を持つ。
//! 符号付きの残差はジグザグ符号化 (folding) で符号なしに変換される。

use alloc::format;
use alloc::vec::Vec;

use crate::bit_reader::BitReader;
use crate::bit_writer::BitWriter;
use crate::error::{DecodeError, ParseError};

/// 残差サンプルは 32 bit 符号付き (最小値除く) に収まらなければならない
/// (RFC 9639 Section 9.2.7.3)。folded 表現では [0, 2^32-2] に対応する
/// (2^32-1 は signed の最負値 -2^31 に対応し RFC で除外されている)
const MAX_FOLDED_RESIDUAL: u64 = (u32::MAX - 1) as u64;

/// Rice パラメータの最大値
///
/// 5 bit パラメータの全ビット 1 (31) はエスケープコードのため、パラメータと
/// して使えるのは 30 まで (RFC 9639 Section 9.2.7)。
const MAX_RICE_PARAMETER: usize = 30;

/// ジグザグ符号化 (folding): 符号付き残差を符号なしへ (RFC 9639 Section 9.2.7.2)
///
/// 正の数は 2 倍、負の数は -2 倍して 1 を引く。RFC の記述は正負の場合分け
/// だが、分岐のない `(value << 1) ^ (value >> 63)` (算術シフト) と等価で、
/// ホットループの自動ベクトル化を妨げないこちらの形で計算する
/// (等価性は fold_matches_rfc_definition テストで確認している)。
/// 呼び出し側の残差は最大でも 37 bit 程度 (RFC 9639 Appendix A.3) なので
/// `value << 1` は溢れない。
#[inline(always)]
fn fold(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// ジグザグ復号 (unfolding): 符号なし folded 残差を符号付きへ
/// (RFC 9639 Section 9.2.7.2)
///
/// 偶数は 1 bit 右シフト、奇数は 1 bit 右シフトして全ビット反転。
/// 分岐のない同値形 (右シフト後に最下位ビットの符号マスクと XOR) で書く。
#[inline(always)]
fn unfold(folded: u64) -> i64 {
    ((folded >> 1) as i64) ^ -((folded & 1) as i64)
}

/// 符号化された残差をデコードして `out` に追記する (RFC 9639 Section 9.2.7)
///
/// `out` には warm-up サンプルが predictor_order 個入っている状態で呼ぶこと。
pub(crate) fn decode_residual(
    reader: &mut BitReader<'_>,
    block_size: u16,
    predictor_order: u32,
    out: &mut Vec<i64>,
) -> Result<(), ParseError> {
    // 符号化方式 (RFC 9639 Section 9.2.7 Table 23)
    let method = reader.read_u32(2)?;
    let parameter_bits = match method {
        0b00 => 4,
        0b01 => 5,
        _ => {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "residual coding method {:#04b} is reserved (RFC 9639 Section 9.2.7)",
                method
            ))));
        }
    };
    // エスケープコードはパラメータビットが全て 1 (RFC 9639 Section 9.2.7)
    let escape_code = (1u32 << parameter_bits) - 1;

    let partition_order = reader.read_u32(4)?;
    let partition_count = 1u32 << partition_order;

    // ブロックサイズはパーティション数で割り切れなければならない
    // (RFC 9639 Section 9.2.7)
    if !u32::from(block_size).is_multiple_of(partition_count) {
        return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
            "block size {} is not divisible into {} partitions (RFC 9639 Section 9.2.7)",
            block_size, partition_count
        ))));
    }
    let partition_samples = u32::from(block_size) >> partition_order;
    // 最初のパーティションのサンプル数は (block size >> partition order) - predictor order
    // であり、正でなければならない (RFC 9639 Section 9.2.7)
    if partition_samples <= predictor_order {
        return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
            "partition sample count {} does not exceed predictor order {} (RFC 9639 Section 9.2.7)",
            partition_samples, predictor_order
        ))));
    }

    for partition in 0..partition_count {
        let sample_count = if partition == 0 {
            partition_samples - predictor_order
        } else {
            partition_samples
        };
        // サンプルごとの push は容量チェックが毎回走るため、パーティション分を
        // まとめて resize してからスライスへ書き込む。全パーティションの合計は
        // ブロックサイズ (16 bit) 以下なので、確保量は最大でも 65535 要素に
        // 有界で、壊れた入力によるメモリ浪費にはならない。途中でエラーに
        // なった場合の余分な 0 は、フレームごと破棄されるため問題にならない
        let start = out.len();
        out.resize(start + sample_count as usize, 0);
        let slots = &mut out[start..];
        let parameter = reader.read_u32(parameter_bits)?;
        if parameter == escape_code {
            // エスケープパーティション: 固定長の未符号化残差 (RFC 9639 Section 9.2.7.1)
            let bits = reader.read_u32(5)?;
            // ビット数 0 は全残差サンプルが 0 であることを表す。resize の
            // 0 埋めがそのまま値になるため読み取りは不要
            if bits > 0 {
                for slot in slots {
                    *slot = reader.read_i64(bits)?;
                }
            }
        } else {
            // Rice 符号 (RFC 9639 Section 9.2.7.2)
            for slot in slots {
                let folded = reader.read_rice(parameter)?;
                // 残差は 32 bit 符号付き (最小値除く) に収まらなければならない
                // (RFC 9639 Section 9.2.7.3)。quotient の読み過ぎによる
                // メモリ浪費もここで防ぐ
                if folded > MAX_FOLDED_RESIDUAL {
                    return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                        "folded residual {} exceeds 32-bit limit (RFC 9639 Section 9.2.7.3)",
                        folded
                    ))));
                }
                *slot = unfold(folded);
            }
        }
    }
    Ok(())
}

/// Rice 符号 1 個を書き出す
///
/// `total_bits` (quotient + 1 + parameter) が 64 bit に収まる場合は 1 回の
/// 書き込みに融合し、収まらない場合だけ単進符号と remainder に分けて書く。
fn write_rice_code(
    writer: &mut BitWriter,
    folded: u64,
    total_bits: u64,
    parameter: u32,
    mask: u64,
    terminator: u64,
) {
    if total_bits <= 64 {
        writer.write_u64(terminator | (folded & mask), total_bits as u32);
    } else {
        writer.write_unary(folded >> parameter);
        writer.write_u64(folded & mask, parameter);
    }
}

/// パーティション 1 個のエンコード方法
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartitionMethod {
    /// Rice パラメータ
    Rice(u32),
    /// エスケープ: 固定長ビット数
    Escape(u32),
}

/// 残差計画で使い回す作業バッファ
///
/// 計画のたびに確保すると malloc/free がエンコード時間の 1 割を占める
/// (実測) ため、呼び出し側が保持して全計画で再利用する。
#[derive(Debug, Default)]
pub(crate) struct RiceScratch {
    /// folded 残差
    folded: Vec<u64>,
    /// 最大パーティションオーダーでの各パーティションの統計
    stats: Vec<PartitionStats>,
    /// 全オーダー分のパーティション方法の蓄積
    ///
    /// オーダーごとに追記し、最良オーダーが確定した後に該当範囲だけを
    /// 切り出す (オーダーごとの Vec 確保を避ける)
    methods: Vec<PartitionMethod>,
}

/// 残差全体のエンコード計画 (RFC 9639 Section 9.2.7)
///
/// エンコードに先立ってパーティション分割とパラメータを決定し、
/// 正確なビット数を見積もれるようにする。
#[derive(Debug, Clone)]
pub(crate) struct ResidualPlan {
    /// パラメータのビット数 (4 または 5) (RFC 9639 Section 9.2.7 Table 23)
    parameter_bits: u32,
    /// パーティションオーダー
    partition_order: u32,
    /// 各パーティションの方法
    partitions: Vec<PartitionMethod>,
    /// 残差部分の合計ビット数 (方式ビット・オーダービット含む)
    total_bits: u64,
    /// folded 残差の最大値 (32 bit 制約の判定用)
    max_folded: u64,
}

/// 1 パーティション分の残差統計
///
/// folded 残差の右シフト和 `sums[k] = Σ(folded >> k)` を全パラメータ分
/// 保持し、全ての Rice パラメータの正確な符号長をパラメータ 1 個あたり
/// O(1) で導出できるようにする。シフト和は分岐のない連続走査で集計できる
/// (自動ベクトル化しやすい形)。パーティションオーダーを下げる際はペアの
/// `merge` で残差の再走査なしに統計を合成できる (リファレンス実装 libFLAC
/// の階層和と同じ考え方)。
#[derive(Debug, Clone)]
struct PartitionStats {
    /// 残差サンプル数
    count: u64,
    /// folded 残差の最大値
    max_folded: u64,
    /// folded 残差の OR 蓄積。「全サンプルが 0」の判定とエスケープの
    /// 必要ビット数の導出に使う (RFC 9639 Section 9.2.7.1)
    folded_or: u64,
    /// sums[k] = Σ(folded >> k)。パラメータ k の quotient の総和
    sums: [u64; MAX_RICE_PARAMETER + 1],
}

impl PartitionStats {
    /// folded 残差のスライスから統計を集める
    fn collect(folded: &[u64]) -> Self {
        // 最大値・OR・総和 (sums[0]) は 1 回の走査で同時に集める。
        // sums[0] は最大でも 65535 サンプル x 2^32 < 2^48 なので
        // オーバーフローしない
        let mut max_folded = 0u64;
        let mut folded_or = 0u64;
        let mut sum0 = 0u64;
        for &value in folded {
            max_folded = max_folded.max(value);
            folded_or |= value;
            sum0 += value;
        }
        let mut sums = [0u64; MAX_RICE_PARAMETER + 1];
        sums[0] = sum0;
        // folded の最上位ビット位置を超える k はシフト結果が全サンプルで 0
        // になるため、総和 0 のまま走査を省く
        let top = ((64 - folded_or.leading_zeros()) as usize).min(sums.len());
        // シフト和はパラメータ 8 本ずつまとめて集計し、folded のロードを
        // 共有する (走査回数が約 1/8 になる)
        let mut k = 1;
        while k + 7 < top {
            let mut s = [0u64; 8];
            for &value in folded {
                s[0] += value >> k;
                s[1] += value >> (k + 1);
                s[2] += value >> (k + 2);
                s[3] += value >> (k + 3);
                s[4] += value >> (k + 4);
                s[5] += value >> (k + 5);
                s[6] += value >> (k + 6);
                s[7] += value >> (k + 7);
            }
            sums[k..k + 8].copy_from_slice(&s);
            k += 8;
        }
        while k + 3 < top {
            let mut s = [0u64; 4];
            for &value in folded {
                s[0] += value >> k;
                s[1] += value >> (k + 1);
                s[2] += value >> (k + 2);
                s[3] += value >> (k + 3);
            }
            sums[k..k + 4].copy_from_slice(&s);
            k += 4;
        }
        while k < top {
            sums[k] = folded.iter().map(|&value| value >> k).sum();
            k += 1;
        }
        Self {
            count: folded.len() as u64,
            max_folded,
            folded_or,
            sums,
        }
    }

    /// 2 つのパーティションの統計を合成する
    fn merge(&self, other: &Self) -> Self {
        let mut sums = self.sums;
        for (acc, value) in sums.iter_mut().zip(other.sums.iter()) {
            *acc += value;
        }
        Self {
            count: self.count + other.count,
            max_folded: self.max_folded.max(other.max_folded),
            folded_or: self.folded_or | other.folded_or,
            sums,
        }
    }
}

/// 統計から 1 パーティション分の最良の方法とビット数を求める
///
/// Rice パラメータ 0 から `max_parameter` までの正確な符号長をシフト和
/// `sums[k]` から導出し、エスケープ (固定長) も含めて最小ビットの方法を
/// 返す。残差を全パラメータで走査し直す全探索と同一の結果を返す。
fn best_partition_method(stats: &PartitionStats, max_parameter: u32) -> (PartitionMethod, u64) {
    let mut best_method = PartitionMethod::Rice(0);
    let mut best_bits = u64::MAX;
    // folded の最上位ビット位置以上のパラメータでは sums が 0 になり、
    // ビット数はパラメータにつき count ずつ単調増加する。探索はそこで
    // 打ち切れる
    let top = 64 - stats.folded_or.leading_zeros();
    for parameter in 0..=max_parameter.min(top) {
        // 各サンプルは quotient (folded >> parameter) + 終端 1 bit +
        // remainder (parameter bit) で符号化される (RFC 9639 Section 9.2.7.2)
        let bits = stats.count * u64::from(parameter + 1) + stats.sums[parameter as usize];
        if bits < best_bits {
            best_bits = bits;
            best_method = PartitionMethod::Rice(parameter);
        } else {
            // ビット数はパラメータに対して離散凸なので、増加に転じたら
            // 以降のパラメータはすべて悪化しかしない。
            // 凸性: bits(k+1) - bits(k) = count - Σ((v >> k) - (v >> (k+1)))
            // で、各項 (v >> k) - (v >> (k+1)) = ceil((v >> k) / 2) は k に
            // ついて単調非増加のため、差分は単調非減少になる
            break;
        }
    }

    // エスケープ (固定長): 全サンプルを表現できる最小ビット数
    // (RFC 9639 Section 9.2.7.1)。残差の大きさ (負は反転) は
    // fold の定義から folded >> 1 と一致し、右シフトは OR と交換できる
    // ため、大きさの OR は folded_or >> 1 で得られる。その最上位ビットは
    // 大きさの最大値のそれと一致するため、必要ビット数 (符号ビット込み)
    // が求まる。
    // 「全サンプルが 0」の判定は folded_or で行う。fold は単射で
    // fold(0) = 0 のため、folded_or == 0 が「全サンプルが 0」と同値になる。
    // -1 (folded = 1) だけのパーティションは folded_or >> 1 = 0 だが
    // folded_or = 1 なので、符号ビットの 1 bit が正しく計上される
    let needed_bits = if stats.folded_or == 0 {
        // 全サンプルが 0 なら「ビット数 0」で表せる
        0
    } else {
        64 - (stats.folded_or >> 1).leading_zeros() + 1
    };
    // エスケープのビット数フィールドは 5 bit なので 31 bit まで
    if needed_bits <= 31 {
        let escape_bits = 5 + u64::from(needed_bits) * stats.count;
        if escape_bits < best_bits {
            best_bits = escape_bits;
            best_method = PartitionMethod::Escape(needed_bits);
        }
    }

    (best_method, best_bits)
}

impl ResidualPlan {
    /// 残差のエンコード計画を立てる
    ///
    /// パーティションオーダー 0 から `max_partition_order` までを試し、
    /// 合計ビット数が最小の分割を選ぶ。残差の走査は 1 パスで、低い
    /// オーダーの統計は最大オーダーのパーティション統計のペア合成で導出する。
    ///
    /// `scratch` は作業バッファで、呼び出し間で使い回す (中身は毎回上書き
    /// される)。
    pub(crate) fn new(
        residual: &[i64],
        block_size: u16,
        predictor_order: u32,
        max_partition_order: u32,
        scratch: &mut RiceScratch,
    ) -> Self {
        debug_assert_eq!(
            residual.len(),
            usize::from(block_size) - predictor_order as usize,
            "残差サンプル数はブロックサイズ - 予測次数 (実装バグ)"
        );

        // 実際に使える最大パーティションオーダーを求める。ブロックサイズが
        // 2^order で割り切れ、かつ最初のパーティションのサンプル数が正である
        // 必要がある (RFC 9639 Section 9.2.7)。この 2 条件はオーダーを下げる
        // 方向に単調なので、以下の全オーダー 0..=effective_max が有効になる
        let mut effective_max = max_partition_order.min(u32::from(block_size).trailing_zeros());
        while effective_max > 0 && (u32::from(block_size) >> effective_max) <= predictor_order {
            effective_max -= 1;
        }

        // folded 残差を 1 パスで前計算し、統計収集を分岐のない連続走査にする
        scratch.folded.clear();
        scratch
            .folded
            .extend(residual.iter().map(|&value| fold(value)));
        let folded: &[u64] = &scratch.folded;

        // 最大オーダーの各パーティションの統計をスライスごとに集める
        let partition_samples = usize::from(block_size) >> effective_max;
        let stats = &mut scratch.stats;
        stats.clear();
        {
            let mut start = 0;
            // 最初のパーティションは予測次数の分だけサンプルが少ない
            let mut end = partition_samples - predictor_order as usize;
            for _ in 0..1usize << effective_max {
                stats.push(PartitionStats::collect(&folded[start..end]));
                start = end;
                end += partition_samples;
            }
        }

        // 5 bit パラメータが必要か判定する。最大 folded 残差が
        // 4 bit パラメータの上限 (14) で表現しきれないほど大きい場合のみ
        // 5 bit を使う (4 bit の方がパラメータの格納が小さい)。
        // Rice 符号の quotient が極端に長くならないよう、パラメータ上限で
        // quotient が 32 bit に収まるかで判定する
        let max_folded = stats.iter().map(|s| s.max_folded).max().unwrap_or(0);
        let parameter_bits = if max_folded >> 14 <= 32 { 4 } else { 5 };
        // エスケープコード (全ビット 1) はパラメータとして使えない
        let max_parameter = (1u32 << parameter_bits) - 2;

        // 高いオーダーから評価し、ペア合成で低いオーダーへ下る。
        // 同点では低いオーダー (ヘッダーが単純な方) を選ぶ。
        // 各オーダーの方法列は methods バッファへ追記し、最良オーダーの
        // 範囲だけを最後に切り出す (オーダーごとの Vec 確保を避ける)
        let methods = &mut scratch.methods;
        methods.clear();
        // (合計ビット数, オーダー, methods 内の開始位置, パーティション数)
        let mut best: Option<(u64, u32, usize, usize)> = None;
        let mut partition_order = effective_max;
        loop {
            // 方式 2 bit + オーダー 4 bit + 各パーティションのパラメータ
            let mut total_bits: u64 = 2 + 4;
            let start = methods.len();
            for stat in stats.iter() {
                let (method, bits) = best_partition_method(stat, max_parameter);
                total_bits = total_bits
                    .saturating_add(u64::from(parameter_bits))
                    .saturating_add(bits);
                methods.push(method);
            }
            if best.is_none_or(|(best_bits, ..)| total_bits <= best_bits) {
                best = Some((total_bits, partition_order, start, stats.len()));
            }
            if partition_order == 0 {
                break;
            }
            // ペアを前詰めで合成し、新しい確保を伴わずに半分の長さへ縮める
            for i in 0..stats.len() / 2 {
                let merged = stats[2 * i].merge(&stats[2 * i + 1]);
                stats[i] = merged;
            }
            let half = stats.len() / 2;
            stats.truncate(half);
            partition_order -= 1;
        }

        let (total_bits, partition_order, start, count) =
            best.expect("パーティションオーダー 0 は常に評価される (実装バグ)");
        ResidualPlan {
            parameter_bits,
            partition_order,
            partitions: methods[start..start + count].to_vec(),
            total_bits,
            max_folded,
        }
    }

    /// この計画で残差をエンコードしたときのビット数 (方式・オーダービット含む)
    pub(crate) fn bits(&self) -> u64 {
        self.total_bits
    }

    /// 全残差サンプルが 32 bit 符号付き (最小値除く) に収まるか
    ///
    /// 収まらない残差は Rice 符号で表現できない (RFC 9639 Section 9.2.7.3)
    /// ため、この計画は使えない。folded 表現 (符号なし) では u32 の範囲に
    /// 収まるかの判定になる。
    pub(crate) fn fits_32bit(&self) -> bool {
        self.max_folded <= MAX_FOLDED_RESIDUAL
    }

    /// 残差をエンコードする
    pub(crate) fn encode(
        &self,
        writer: &mut BitWriter,
        residual: &[i64],
        block_size: u16,
        predictor_order: u32,
    ) {
        // 符号化方式 (RFC 9639 Section 9.2.7 Table 23)
        writer.write_u32(if self.parameter_bits == 4 { 0b00 } else { 0b01 }, 2);
        writer.write_u32(self.partition_order, 4);

        let partition_samples = u32::from(block_size) >> self.partition_order;
        let mut offset = 0usize;
        for (partition, method) in self.partitions.iter().enumerate() {
            let sample_count = if partition == 0 {
                (partition_samples - predictor_order) as usize
            } else {
                partition_samples as usize
            };
            let samples = &residual[offset..offset + sample_count];
            match method {
                PartitionMethod::Rice(parameter) => {
                    let parameter = *parameter;
                    writer.write_u32(parameter, self.parameter_bits);
                    // quotient 個の 0 + 終端の 1 + remainder のビット列は
                    // 値 (1 << parameter) | remainder を quotient + 1 +
                    // parameter ビットで書くのと同一。さらに隣接 2 サンプルの
                    // 符号が合計 64 bit に収まる間は 1 回の書き込みへ融合し、
                    // ライター呼び出し (アキュムレータ更新と満杯判定) を
                    // ほぼ半減させる
                    let terminator = 1u64 << parameter;
                    let mask = terminator - 1;
                    let mut pairs = samples.chunks_exact(2);
                    for pair in &mut pairs {
                        let folded0 = fold(pair[0]);
                        let folded1 = fold(pair[1]);
                        let bits0 = (folded0 >> parameter) + 1 + u64::from(parameter);
                        let bits1 = (folded1 >> parameter) + 1 + u64::from(parameter);
                        if bits0 + bits1 <= 64 {
                            let code0 = terminator | (folded0 & mask);
                            let code1 = terminator | (folded1 & mask);
                            writer.write_u64((code0 << bits1) | code1, (bits0 + bits1) as u32);
                        } else {
                            write_rice_code(writer, folded0, bits0, parameter, mask, terminator);
                            write_rice_code(writer, folded1, bits1, parameter, mask, terminator);
                        }
                    }
                    for &value in pairs.remainder() {
                        let folded = fold(value);
                        let bits = (folded >> parameter) + 1 + u64::from(parameter);
                        write_rice_code(writer, folded, bits, parameter, mask, terminator);
                    }
                }
                PartitionMethod::Escape(bits) => {
                    // エスケープコード (全ビット 1) + 5 bit のビット数
                    writer.write_u32((1 << self.parameter_bits) - 1, self.parameter_bits);
                    writer.write_u32(*bits, 5);
                    for &value in samples {
                        if *bits > 0 {
                            writer.write_i64(value, *bits);
                        }
                    }
                }
            }
            offset += sample_count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_unfold_roundtrip() {
        for value in [
            0i64,
            1,
            -1,
            2,
            -2,
            12345,
            -12345,
            i64::from(i32::MAX),
            i64::from(i32::MIN) + 1,
        ] {
            assert_eq!(unfold(fold(value)), value, "value {}", value);
        }
    }

    /// 分岐なしの unfold が RFC 9639 Section 9.2.7.2 の場合分けの定義と一致する
    #[test]
    fn unfold_matches_rfc_definition() {
        // RFC の記述どおりの場合分け実装 (検証用リファレンス)
        fn unfold_rfc(folded: u64) -> i64 {
            if folded & 1 == 0 {
                (folded >> 1) as i64
            } else {
                !((folded >> 1) as i64)
            }
        }
        for folded in [0u64, 1, 2, 3, 38, 1000, u64::from(u32::MAX), 1 << 40] {
            assert_eq!(unfold(folded), unfold_rfc(folded), "folded {}", folded);
        }
    }

    /// 分岐なしの fold が RFC 9639 Section 9.2.7.2 の場合分けの定義と一致する
    #[test]
    fn fold_matches_rfc_definition() {
        // RFC の記述どおりの場合分け実装 (検証用リファレンス)
        fn fold_rfc(value: i64) -> u64 {
            if value >= 0 {
                (value as u64) << 1
            } else {
                ((-(value + 1)) as u64) << 1 | 1
            }
        }
        // 残差が取り得る範囲 (37 bit 程度) を超える境界値も含めて照合する
        for value in [
            0i64,
            1,
            -1,
            2,
            -2,
            12345,
            -12345,
            i64::from(i32::MAX),
            i64::from(i32::MIN),
            (1i64 << 37) - 1,
            -(1i64 << 37),
        ] {
            assert_eq!(fold(value), fold_rfc(value), "value {}", value);
        }
    }

    /// RFC 9639 Section 9.2.7.2 の例: folded 38、パラメータ 3 の Rice 符号は
    /// 0b00001110 (quotient 4 の unary + remainder 6)
    #[test]
    fn rice_code_rfc9639_example() {
        let mut writer = BitWriter::new();
        // folded 38 = 19 * 2 なので元の値は 19
        assert_eq!(fold(19), 38);
        writer.write_unary(38 >> 3);
        writer.write_u64(38 & 7, 3);
        assert_eq!(writer.into_bytes(), [0b0000_1110]);
    }

    /// RFC 9639 Appendix D.2 Table 38-39 の残差: Rice パラメータ 11、
    /// partition order 0 で 15 サンプル
    #[test]
    fn decode_residual_rfc9639_appendix_d2() {
        // 0x92+1 から始まる符号化残差を手で構築する
        let mut writer = BitWriter::new();
        writer.write_u32(0b00, 2); // 4 bit パラメータの Rice 符号
        writer.write_u32(0, 4); // partition order 0
        writer.write_u32(11, 4); // Rice パラメータ 11
        let folded_values: [(u64, u64); 15] = [
            (3, 244),
            (1, 545),
            (1, 408),
            (0, 1885),
            (0, 1904),
            (0, 1391),
            (0, 1536),
            (0, 1047),
            (0, 1198),
            (0, 801),
            (12, 1767),
            (0, 631),
            (0, 548),
            (0, 533),
            (0, 268),
        ];
        for (quotient, remainder) in folded_values {
            writer.write_unary(quotient);
            writer.write_u64(remainder, 11);
        }
        let bytes = writer.into_bytes();

        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        decode_residual(&mut reader, 16, 1, &mut out).expect("残差デコードに成功するはず");
        assert_eq!(
            out,
            [
                3194, -1297, 1228, -943, 952, -696, 768, -524, 599, -401, -13172, -316, 274, -267,
                134
            ]
        );
    }

    #[test]
    fn decode_rejects_reserved_method() {
        let mut writer = BitWriter::new();
        writer.write_u32(0b10, 2); // 予約された方式
        writer.write_u32(0, 4);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        assert!(matches!(
            decode_residual(&mut reader, 16, 0, &mut out),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_indivisible_partition() {
        let mut writer = BitWriter::new();
        writer.write_u32(0b00, 2);
        writer.write_u32(1, 4); // 2 パーティション
        writer.write_u32(0, 4);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        // ブロックサイズ 15 (奇数) は 2 分割できない
        assert!(matches!(
            decode_residual(&mut reader, 15, 0, &mut out),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_partition_not_exceeding_order() {
        let mut writer = BitWriter::new();
        writer.write_u32(0b00, 2);
        writer.write_u32(2, 4); // 4 パーティション → 16/4 = 4 サンプル
        writer.write_u32(0, 4);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        // 予測次数 4 はパーティションサンプル数 4 以上なので不正
        assert!(matches!(
            decode_residual(&mut reader, 16, 4, &mut out),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn escaped_partition_roundtrip() {
        // エスケープパーティションを含む計画を強制的に作ってラウンドトリップ
        let residual: Vec<i64> = alloc::vec![-10, -6, 2, 8, 8, 6, 0, -3];
        let plan = ResidualPlan::new(&residual, 8, 0, 0, &mut RiceScratch::default());
        let mut writer = BitWriter::new();
        plan.encode(&mut writer, &residual, 8, 0);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        decode_residual(&mut reader, 8, 0, &mut out).expect("残差デコードに成功するはず");
        assert_eq!(out, residual);
    }

    #[test]
    fn residual_plan_roundtrip_with_partitions() {
        // 大きめの残差 (パラメータが偏る) で複数パーティションをテスト
        let mut residual: Vec<i64> = Vec::new();
        for i in 0..256i64 {
            // 前半は小さい値、後半は大きい値
            if i < 128 {
                residual.push((i % 5) - 2);
            } else {
                residual.push((i % 1000) * 17 - 500);
            }
        }
        let plan = ResidualPlan::new(&residual, 256, 0, 4, &mut RiceScratch::default());
        let mut writer = BitWriter::new();
        plan.encode(&mut writer, &residual, 256, 0);
        let bytes = writer.into_bytes();
        // ビット数の見積もりが実際の書き込みと一致する
        assert_eq!(bytes.len(), (plan.bits() as usize).div_ceil(8));
        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        decode_residual(&mut reader, 256, 0, &mut out).expect("残差デコードに成功するはず");
        assert_eq!(out, residual);
    }

    #[test]
    fn residual_plan_with_predictor_order() {
        // 予測次数がある場合の最初のパーティションのサンプル数を確認
        let residual: Vec<i64> = (0..252).map(|i| i64::from(i % 7) - 3).collect();
        let plan = ResidualPlan::new(&residual, 256, 4, 3, &mut RiceScratch::default());
        let mut writer = BitWriter::new();
        plan.encode(&mut writer, &residual, 256, 4);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        decode_residual(&mut reader, 256, 4, &mut out).expect("残差デコードに成功するはず");
        assert_eq!(out, residual);
    }

    #[test]
    fn large_residual_uses_5bit_parameters() {
        // 32 bit 近い残差では 5 bit パラメータが必要になる
        let residual: Vec<i64> = (0..16).map(|i| (i64::from(i) - 8) * 100_000_000).collect();
        let plan = ResidualPlan::new(&residual, 16, 0, 0, &mut RiceScratch::default());
        let mut writer = BitWriter::new();
        plan.encode(&mut writer, &residual, 16, 0);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        let mut out = Vec::new();
        decode_residual(&mut reader, 16, 0, &mut out).expect("残差デコードに成功するはず");
        assert_eq!(out, residual);
    }

    /// 素朴な全探索による 1 パーティションの最良ビット数 (検証用リファレンス)
    fn brute_force_partition_bits(residual: &[i64], max_parameter: u32) -> u64 {
        let mut best = u64::MAX;
        for parameter in 0..=max_parameter {
            let bits: u64 = residual
                .iter()
                .map(|&v| (fold(v) >> parameter) + 1 + u64::from(parameter))
                .sum();
            best = best.min(bits);
        }
        let needed = residual
            .iter()
            .map(|&v| {
                if v == 0 {
                    0
                } else {
                    let magnitude = if v >= 0 { v } else { !v };
                    64 - (magnitude as u64).leading_zeros() + 1
                }
            })
            .max()
            .unwrap_or(0);
        if needed <= 31 {
            best = best.min(5 + u64::from(needed) * residual.len() as u64);
        }
        best
    }

    /// ヒストグラム方式の統計計算が素朴な全探索と同一のビット数を返す
    #[test]
    fn stats_method_matches_brute_force() {
        // 小さい値・大きい値・ゼロ・負値が混ざる決定的な残差列
        let mut state = 0xDEADBEEFu64;
        for len in [1usize, 7, 64, 255] {
            let residual: Vec<i64> = (0..len)
                .map(|i| {
                    state = state
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    match i % 5 {
                        0 => 0,
                        1 => (state >> 60) as i64 - 8,
                        2 => (state >> 48) as i64 - 32768,
                        3 => -((state >> 40) as i64),
                        _ => (state >> 33) as i64,
                    }
                })
                .collect();
            for max_parameter in [14u32, 30] {
                let folded: Vec<u64> = residual.iter().map(|&v| fold(v)).collect();
                let stats = PartitionStats::collect(&folded);
                let (_, bits) = best_partition_method(&stats, max_parameter);
                assert_eq!(
                    bits,
                    brute_force_partition_bits(&residual, max_parameter),
                    "長さ {} パラメータ上限 {} で不一致",
                    len,
                    max_parameter
                );
            }
        }
    }

    /// magnitude が 0 になる -1 を含むパーティションの境界ケース
    ///
    /// -1 の magnitude (!(-1)) は 0 のため、「全サンプルが 0」との区別を
    /// 誤ると -1 がエスケープビット数 0 で消えてしまう。
    #[test]
    fn stats_method_handles_minus_one_and_zero() {
        for residual in [
            alloc::vec![0i64, 0, 0, 0, -1],
            alloc::vec![-1i64],
            alloc::vec![-1i64, -1, -1],
            alloc::vec![0i64],
            alloc::vec![0i64, 0, 0],
        ] {
            let folded: Vec<u64> = residual.iter().map(|&v| fold(v)).collect();
            let stats = PartitionStats::collect(&folded);
            let (method, bits) = best_partition_method(&stats, 14);
            assert_eq!(
                bits,
                brute_force_partition_bits(&residual, 14),
                "残差 {:?} で不一致",
                residual
            );
            // -1 を含む場合はビット数 0 のエスケープになってはならない
            if residual.contains(&-1) {
                assert_ne!(method, PartitionMethod::Escape(0), "残差 {:?}", residual);
            }
        }
    }

    /// 統計のペア合成が「まとめて集計」と同じ結果になる
    #[test]
    fn stats_merge_matches_combined() {
        let left: Vec<i64> = (0..37).map(|i| i * 31 - 500).collect();
        let right: Vec<i64> = (0..41).map(|i| -(i * 17) + 123).collect();

        let folded_left: Vec<u64> = left.iter().map(|&v| fold(v)).collect();
        let folded_right: Vec<u64> = right.iter().map(|&v| fold(v)).collect();
        let folded_all: Vec<u64> = folded_left
            .iter()
            .chain(folded_right.iter())
            .copied()
            .collect();

        let stats_left = PartitionStats::collect(&folded_left);
        let stats_right = PartitionStats::collect(&folded_right);
        let stats_all = PartitionStats::collect(&folded_all);
        let merged = stats_left.merge(&stats_right);
        assert_eq!(merged.count, stats_all.count);
        assert_eq!(merged.max_folded, stats_all.max_folded);
        assert_eq!(merged.folded_or, stats_all.folded_or);
        assert_eq!(merged.sums, stats_all.sums);
    }
}
