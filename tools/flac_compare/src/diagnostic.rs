//! FLAC ビットストリームの診断パーサーと、本家 flac -a の分析出力との照合
//!
//! エンコーダーの圧縮率調査のため、フレームごとのサブフレーム符号化条件
//! (パーティションオーダー・Rice パラメータ・予測方式・wasted bits・
//! チャンネル割り当て・LPC 係数精度・量子化シフト) をビットストリームから
//! 抽出する。ライブラリの private / `pub(crate)` API を使わず、
//! RFC 9639 のビットレイアウトを直接読む。
//!
//! 照合は「両方に出る項目だけ」で行う。本家 `flac -a` の分析出力に現れない
//! 項目 (warm-up サンプル値など) は照合に使わない。Rice パラメータの
//! 4 bit / 5 bit 方式は本家が `residual_type` (RICE / RICE2) として出力する
//! ため、照合に含める。

use std::fmt;

/// フレーム 1 個分の診断情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameDiag {
    /// ブロックサイズ (インターチャンネルサンプル数)
    pub(crate) block_size: u32,
    /// サンプルレート (Hz)。フレームヘッダーに無い場合は 0
    pub(crate) sample_rate: u32,
    /// チャンネル割り当ての表記 (本家 flac -a と同じ名前)
    pub(crate) channel_assignment: &'static str,
    /// サブフレームの診断情報 (チャンネル数分)
    pub(crate) subframes: Vec<SubframeDiag>,
}

/// サブフレーム 1 個分の診断情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubframeDiag {
    /// wasted bits の数 (RFC 9639 Section 9.2.2)
    pub(crate) wasted_bits: u32,
    /// サブフレームの種類とパラメータ
    pub(crate) kind: SubframeKind,
    /// 残差の診断情報 (CONSTANT / VERBATIM には無い)
    pub(crate) residual: Option<ResidualDiag>,
}

/// サブフレームの種類 (RFC 9639 Section 9.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubframeKind {
    /// CONSTANT (RFC 9639 Section 9.2.3)
    Constant,
    /// VERBATIM (RFC 9639 Section 9.2.4)
    Verbatim,
    /// FIXED: 固定予測、次数付き (RFC 9639 Section 9.2.5)
    Fixed { order: u32 },
    /// LPC: 線形予測、次数・係数精度・量子化シフト付き (RFC 9639 Section 9.2.6)
    Lpc {
        order: u32,
        precision: u32,
        /// 量子化シフト。本家 flac -a の `quantization_level` は符号付きで
        /// 出力される (負値は RFC では禁止だが、他実装の出力を照合するため
        /// i64 で持つ)
        shift: i64,
    },
}

/// 残差の診断情報 (RFC 9639 Section 9.2.7)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResidualDiag {
    /// Rice パラメータのビット数 (4 または 5)。
    /// 本家 flac -a の出力には現れないため、flac-rs 側のパースでのみ
    /// Some になる (照合には使わない)
    pub(crate) parameter_bits: Option<u32>,
    /// パーティションオーダー
    pub(crate) partition_order: u32,
    /// 各パーティションの符号化方法
    pub(crate) partitions: Vec<PartitionDiag>,
}

/// パーティション 1 個の符号化方法 (RFC 9639 Section 9.2.7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartitionDiag {
    /// Rice パラメータ
    Rice(u32),
    /// エスケープ: 固定長ビット数
    Escape(u32),
}

impl fmt::Display for PartitionDiag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PartitionDiag::Rice(parameter) => write!(f, "{parameter}"),
            PartitionDiag::Escape(bits) => write!(f, "ESCAPE, raw_bits={bits}"),
        }
    }
}

/// フレーム番号とサブフレーム番号の組
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Position {
    frame: usize,
    subframe: usize,
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame={} subframe={}", self.frame, self.subframe)
    }
}

/// 2 つの診断列をサブフレーム単位で照合し、差分メッセージを返す
///
/// 本家 flac -a の分析出力に現れない項目 (warm-up サンプル値など) は
/// 比較しない。戻り値が空なら完全一致。
pub(crate) fn compare(rs: &[FrameDiag], reference: &[FrameDiag]) -> Vec<String> {
    let mut diffs = Vec::new();
    if rs.len() != reference.len() {
        diffs.push(format!(
            "frame count mismatch: flac-rs {} frames, reference {} frames",
            rs.len(),
            reference.len()
        ));
    }
    for (i, (rs_frame, ref_frame)) in rs.iter().zip(reference.iter()).enumerate() {
        compare_frame(&mut diffs, i, rs_frame, ref_frame);
    }
    diffs
}

/// フレーム 1 個分を照合する
fn compare_frame(diffs: &mut Vec<String>, frame: usize, rs: &FrameDiag, reference: &FrameDiag) {
    if rs.block_size != reference.block_size {
        diffs.push(format!(
            "frame={frame}: block size mismatch: flac-rs {}, reference {}",
            rs.block_size, reference.block_size
        ));
    }
    if rs.sample_rate != 0 && reference.sample_rate != 0 && rs.sample_rate != reference.sample_rate
    {
        diffs.push(format!(
            "frame={frame}: sample rate mismatch: flac-rs {}, reference {}",
            rs.sample_rate, reference.sample_rate
        ));
    }
    if rs.channel_assignment != reference.channel_assignment {
        diffs.push(format!(
            "frame={frame}: channel assignment mismatch: flac-rs {}, reference {}",
            rs.channel_assignment, reference.channel_assignment
        ));
    }
    if rs.subframes.len() != reference.subframes.len() {
        diffs.push(format!(
            "frame={frame}: subframe count mismatch: flac-rs {}, reference {}",
            rs.subframes.len(),
            reference.subframes.len()
        ));
    }
    for (j, (rs_sub, ref_sub)) in rs
        .subframes
        .iter()
        .zip(reference.subframes.iter())
        .enumerate()
    {
        compare_subframe(diffs, Position { frame, subframe: j }, rs_sub, ref_sub);
    }
}

/// サブフレーム 1 個分を照合する
fn compare_subframe(
    diffs: &mut Vec<String>,
    pos: Position,
    rs: &SubframeDiag,
    reference: &SubframeDiag,
) {
    if rs.wasted_bits != reference.wasted_bits {
        diffs.push(format!(
            "{pos}: wasted bits mismatch: flac-rs {}, reference {}",
            rs.wasted_bits, reference.wasted_bits
        ));
    }
    if rs.kind != reference.kind {
        diffs.push(format!(
            "{pos}: subframe kind mismatch: flac-rs {:?}, reference {:?}",
            rs.kind, reference.kind
        ));
    }
    match (&rs.residual, &reference.residual) {
        (Some(rs_res), Some(ref_res)) => {
            // Rice パラメータの方式 (4 bit / 5 bit) は本家 .ana の
            // residual_type (RICE / RICE2) から分かるため、照合に含める
            if rs_res.parameter_bits != ref_res.parameter_bits {
                diffs.push(format!(
                    "{pos}: rice parameter bits mismatch: flac-rs {:?}, reference {:?}",
                    rs_res.parameter_bits, ref_res.parameter_bits
                ));
            }
            if rs_res.partition_order != ref_res.partition_order {
                diffs.push(format!(
                    "{pos}: partition order mismatch: flac-rs {}, reference {}",
                    rs_res.partition_order, ref_res.partition_order
                ));
                // パーティション分割が異なる時点で個別パーティションの比較は
                // 意味をなさず、差分が膨大になるため、ここで打ち切る
                return;
            }
            if rs_res.partitions.len() != ref_res.partitions.len() {
                diffs.push(format!(
                    "{pos}: partition count mismatch: flac-rs {}, reference {}",
                    rs_res.partitions.len(),
                    ref_res.partitions.len()
                ));
                return;
            }
            for (k, (rs_part, ref_part)) in rs_res
                .partitions
                .iter()
                .zip(ref_res.partitions.iter())
                .enumerate()
            {
                if rs_part != ref_part {
                    diffs.push(format!(
                        "{pos} partition {k}: parameter mismatch: flac-rs {rs_part}, reference {ref_part}"
                    ));
                }
            }
        }
        (Some(_), None) => {
            diffs.push(format!("{pos}: flac-rs has a residual, reference does not"));
        }
        (None, Some(_)) => {
            diffs.push(format!("{pos}: reference has a residual, flac-rs does not"));
        }
        (None, None) => {}
    }
}

/// バイト列のビットリーダー (診断パーサー用の最小実装)
struct BitReader<'a> {
    data: &'a [u8],
    /// 次に読むビット位置 (先頭から数えたビット数)
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// バイト列からビットリーダーを作る
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    /// 現在のビット位置 (先頭からのビット数) を返す
    fn pos(&self) -> usize {
        self.bit_pos
    }

    /// 1 bit を読む
    fn read_bit(&mut self) -> Result<u32, String> {
        if self.bit_pos >= self.data.len() * 8 {
            return Err("bitstream is truncated".to_string());
        }
        let byte = self.data[self.bit_pos / 8];
        let bit = (byte >> (7 - self.bit_pos % 8)) & 1;
        self.bit_pos += 1;
        Ok(u32::from(bit))
    }

    /// `bits` (最大 32) ビットをビッグエンディアンで読む
    fn read_bits(&mut self, bits: u32) -> Result<u32, String> {
        debug_assert!(bits <= 32);
        let mut value = 0u32;
        for _ in 0..bits {
            value = (value << 1) | self.read_bit()?;
        }
        Ok(value)
    }

    /// `bits` ビットを読み飛ばす
    fn skip(&mut self, bits: usize) -> Result<(), String> {
        let end = self
            .bit_pos
            .checked_add(bits)
            .ok_or_else(|| "bit position overflow".to_string())?;
        if end > self.data.len() * 8 {
            return Err("bitstream is truncated".to_string());
        }
        self.bit_pos = end;
        Ok(())
    }

    /// unary 符号を読み、0 の個数 (終端の 1 を含まない) を返す
    fn read_unary(&mut self) -> Result<u32, String> {
        let mut count = 0u32;
        loop {
            if self.read_bit()? == 1 {
                return Ok(count);
            }
            // 2^32 個の 0 はデータ長 (最大でデータの 8 倍) を超えるため、
            // 通常は read_bit の truncated チェックが先に発火する。防御として
            // カウンタのオーバーフローも検査する
            count = count
                .checked_add(1)
                .ok_or_else(|| "unary count overflow".to_string())?;
        }
    }
}

/// 最初のフレームのバイト位置を返す
///
/// fLaC マーカー (4 バイト) の後にメタデータブロックが続き、last フラグが
/// 立ったブロックの直後に最初のフレームが始まる (RFC 9639 Section 8.1)。
/// メタデータ走査は `crate::frame_data_size` と共通のため、フレームデータの
/// バイト数から逆算する。
fn first_frame_offset(data: &[u8]) -> Result<usize, String> {
    let frame_bytes = crate::frame_data_size(data)?;
    Ok(data.len() - frame_bytes)
}

/// FLAC ストリーム全体を診断し、フレームごとの情報を返す
pub(crate) fn analyze(data: &[u8]) -> Result<Vec<FrameDiag>, String> {
    let mut reader = BitReader::new(data);
    let frame_start = first_frame_offset(data)?;
    reader.skip(frame_start * 8)?;
    let mut frames = Vec::new();
    while reader.pos() < data.len() * 8 {
        frames.push(parse_frame(&mut reader)?);
    }
    Ok(frames)
}

/// フレーム 1 個分をパースする (フレームは同期コードで始まる)
fn parse_frame(reader: &mut BitReader) -> Result<FrameDiag, String> {
    // フレーム同期コード (RFC 9639 Section 9.1)
    let sync = reader.read_bits(15)?;
    if sync != 0b111111111111100 {
        return Err(format!(
            "frame sync mismatch: expected 0b111111111111100, got {sync:#018b}"
        ));
    }
    // blocking strategy 1 bit (固定 / 可変。診断では使い分けない)
    reader.read_bit()?;

    // ブロックサイズ (RFC 9639 Section 9.1.1 Table 14)
    let block_size_bits = reader.read_bits(4)?;
    let block_size = match block_size_bits {
        0b0001 => 192,
        0b0010..=0b0101 => 144 * (1 << block_size_bits),
        0b1000..=0b1111 => 1 << block_size_bits,
        // 0b0110 / 0b0111 の uncommon block size は coded number の後に
        // 格納される (RFC 9639 Section 9.1.6)。後で読み取る
        0b0110 | 0b0111 => 0,
        _ => return Err("reserved block size bits".to_string()),
    };

    // サンプルレート (RFC 9639 Section 9.1.2 Table 15)
    let sample_rate_bits = reader.read_bits(4)?;
    let sample_rate = match sample_rate_bits {
        // 0b0000 (streaminfo 参照) はヘッダーに値が無く、以降のパースに
        // サンプルレートが必要にならないためエラーにする (照合もできない)
        0b0000 => return Err("sample rate is only stored in streaminfo".to_string()),
        0b0001 => 88200,
        0b0010 => 176400,
        0b0011 => 192000,
        0b0100 => 8000,
        0b0101 => 16000,
        0b0110 => 22050,
        0b0111 => 24000,
        0b1000 => 32000,
        0b1001 => 44100,
        0b1010 => 48000,
        0b1011 => 96000,
        // 0b1100 / 0b1101 / 0b1110 の uncommon sample rate は coded number の
        // 後に格納される (RFC 9639 Section 9.1.7)。後で読み取る
        0b1100..=0b1110 => 0,
        _ => return Err("forbidden sample rate bits".to_string()),
    };

    // チャンネル割り当て (RFC 9639 Section 9.1.3 Table 16)
    let channels_bits = reader.read_bits(4)?;
    let (channels, channel_assignment) = match channels_bits {
        0b0000..=0b0111 => (channels_bits + 1, "INDEPENDENT"),
        0b1000 => (2, "LEFT_SIDE"),
        0b1001 => (2, "RIGHT_SIDE"),
        0b1010 => (2, "MID_SIDE"),
        // 0b1011-0b1111 は予約済み (RFC 9639 Section 9.1.3 Table 16)
        _ => return Err("reserved channel assignment".to_string()),
    };

    // ビット深度 (RFC 9639 Section 9.1.4 Table 17)
    let bit_depth_bits = reader.read_bits(3)?;
    let bits_per_sample = match bit_depth_bits {
        // 0b000 (streaminfo 参照) はヘッダーに値が無く、サブフレームの
        // 読み飛ばし量が決まらないためエラーにする
        0b000 => return Err("bit depth is only stored in streaminfo".to_string()),
        0b001 => 8,
        0b010 => 12,
        0b100 => 16,
        0b101 => 20,
        0b110 => 24,
        0b111 => 32,
        _ => return Err("reserved bit depth bits".to_string()),
    };
    // 予約ビット (MUST be 0) (RFC 9639 Section 9.1.4)
    if reader.read_bit()? != 0 {
        return Err("reserved bit after bit depth bits must be zero".to_string());
    }

    // フレーム / サンプル番号 (UTF-8 ライクな可変長、RFC 9639 Section 9.1.5)。
    // 値は使わないので最初のバイトから長さだけを判定して読み飛ばす
    let first = reader.read_bits(8)?;
    let bytes = if first & 0b1000_0000 == 0 {
        1
    } else if first & 0b1110_0000 == 0b1100_0000 {
        2
    } else if first & 0b1111_0000 == 0b1110_0000 {
        3
    } else if first & 0b1111_1000 == 0b1111_0000 {
        4
    } else if first & 0b1111_1100 == 0b1111_1000 {
        5
    } else if first & 0b1111_1110 == 0b1111_1100 {
        6
    } else {
        7
    };
    if bytes > 1 {
        reader.skip((bytes - 1) * 8)?;
    }

    // uncommon block size (RFC 9639 Section 9.1.6)。coded number の後に
    // ブロックサイズ - 1 が 8 bit または 16 bit で格納される
    let block_size = if block_size == 0 {
        let stored = match block_size_bits {
            0b0110 => reader.read_bits(8)?,
            0b0111 => reader.read_bits(16)?,
            _ => return Err("uncommon block size bits are not set".to_string()),
        };
        // 65535 (ブロックサイズ 65536) は禁止 (RFC 9639 Section 9.1.6)
        if stored == 0xFFFF {
            return Err(
                "uncommon block size 65536 is forbidden (RFC 9639 Section 9.1.6)".to_string(),
            );
        }
        stored + 1
    } else {
        block_size
    };

    // uncommon sample rate (RFC 9639 Section 9.1.7)。uncommon block size の
    // 後に 8 bit (kHz) または 16 bit (Hz / Hz÷10) で格納される
    let sample_rate = if sample_rate == 0 {
        match sample_rate_bits {
            0b1100 => reader.read_bits(8)? * 1000,
            0b1101 => reader.read_bits(16)?,
            0b1110 => reader.read_bits(16)? * 10,
            // 0b0000 (streaminfo 参照) はヘッダーに値が無いため 0 のまま
            _ => 0,
        }
    } else {
        sample_rate
    };

    // フレームヘッダー CRC-8 (RFC 9639 Section 9.1.8)。値は照合しない
    reader.skip(8)?;

    // サブフレームをチャンネル数分パースする
    let mut subframes = Vec::with_capacity(channels as usize);
    for channel in 0..channels {
        // サイドチャンネルはビット深度が 1 bit 増える (RFC 9639 Section 9.1.3)。
        // LEFT_SIDE / MID_SIDE は 2 番目、RIGHT_SIDE は 1 番目が side
        let is_side = (channel_assignment == "LEFT_SIDE" || channel_assignment == "MID_SIDE")
            && channel == 1
            || channel_assignment == "RIGHT_SIDE" && channel == 0;
        let subframe_bits = if is_side && bits_per_sample > 0 {
            bits_per_sample + 1
        } else {
            bits_per_sample
        };
        subframes.push(parse_subframe(reader, block_size, subframe_bits)?);
    }

    // サブフレームの後にバイト境界まで 0 詰めされ、フレーム全体の CRC-16
    // が続く (RFC 9639 Section 9.3)。パディングを飛ばして CRC を読み捨てる
    reader.skip((8 - reader.pos() % 8) % 8)?;
    reader.skip(16)?;

    Ok(FrameDiag {
        block_size,
        sample_rate,
        channel_assignment,
        subframes,
    })
}

/// サブフレーム 1 個分をパースする (RFC 9639 Section 9.2)
///
/// `bits_per_sample` はこのサブフレームのビット深度 (サイドチャンネルの +1 を
/// 適用済み、0 は streaminfo 依存でヘッダーに無い場合)。
fn parse_subframe(
    reader: &mut BitReader,
    block_size: u32,
    bits_per_sample: u32,
) -> Result<SubframeDiag, String> {
    // サブフレームヘッダー (RFC 9639 Section 9.2.1)
    let pad = reader.read_bit()?;
    if pad != 0 {
        return Err("subframe header must start with a zero bit".to_string());
    }
    let subframe_type = reader.read_bits(6)?;

    // wasted bits (RFC 9639 Section 9.2.2)
    let wasted_bits = if reader.read_bit()? == 1 {
        reader.read_unary()? + 1
    } else {
        0
    };
    // 適用後のビット深度は 1 以上でなければならない (RFC 9639 Section 9.2.2)
    if wasted_bits >= bits_per_sample {
        return Err(format!(
            "wasted bits {wasted_bits} leaves no bits for samples of depth {bits_per_sample} (RFC 9639 Section 9.2.2)"
        ));
    }
    let coded_bits = bits_per_sample - wasted_bits;

    let kind = match subframe_type {
        // CONSTANT (RFC 9639 Section 9.2.3)
        0b000000 => {
            reader.skip(coded_bits as usize)?;
            SubframeKind::Constant
        }
        // VERBATIM (RFC 9639 Section 9.2.4)
        0b000001 => {
            reader.skip(coded_bits as usize * block_size as usize)?;
            SubframeKind::Verbatim
        }
        // FIXED (RFC 9639 Section 9.2.5)
        0b001000..=0b001100 => {
            let order = subframe_type - 0b001000;
            // warm-up サンプル (RFC 9639 Section 9.2.5 Table 21)
            reader.skip(coded_bits as usize * order as usize)?;
            SubframeKind::Fixed { order }
        }
        // LPC (RFC 9639 Section 9.2.6)
        0b100000..=0b111111 => {
            let order = subframe_type - 31;
            // warm-up サンプル (RFC 9639 Section 9.2.6 Table 22)
            reader.skip(coded_bits as usize * order as usize)?;
            // 係数精度 - 1 (RFC 9639 Section 9.2.6)
            let precision_bits = reader.read_bits(4)?;
            if precision_bits == 0b1111 {
                return Err("predictor coefficient precision bits 0b1111 is forbidden".to_string());
            }
            let precision = precision_bits + 1;
            // 予測右シフト (RFC 9639 Section 9.2.6)。5 bit の符号付きで、
            // 負の値は禁止 (MUST NOT be negative)
            let shift_bits = reader.read_bits(5)?;
            if shift_bits >= 16 {
                return Err(format!(
                    "negative prediction right shift {} is forbidden (RFC 9639 Section 9.2.6)",
                    (shift_bits as i64) - 32
                ));
            }
            let shift = i64::from(shift_bits);
            // 係数 (ビットストリーム順)
            reader.skip(precision as usize * order as usize)?;
            SubframeKind::Lpc {
                order,
                precision,
                shift,
            }
        }
        _ => return Err(format!("reserved subframe type {subframe_type:#08b}")),
    };

    // CONSTANT / VERBATIM 以外は残差を持つ (RFC 9639 Section 9.2.7)
    let residual = match kind {
        SubframeKind::Constant | SubframeKind::Verbatim => None,
        SubframeKind::Fixed { order } => Some(parse_residual(reader, block_size, order)?),
        SubframeKind::Lpc { order, .. } => Some(parse_residual(reader, block_size, order)?),
    };

    Ok(SubframeDiag {
        wasted_bits,
        kind,
        residual,
    })
}

/// 残差をパースする (RFC 9639 Section 9.2.7)
///
/// パーティションごとのパラメータとエスケープの固定長を記録し、残差データ
/// 本体は読み飛ばす。
fn parse_residual(
    reader: &mut BitReader,
    block_size: u32,
    predictor_order: u32,
) -> Result<ResidualDiag, String> {
    // 符号化方式 (RFC 9639 Section 9.2.7 Table 23)
    let method = reader.read_bits(2)?;
    let parameter_bits = match method {
        0b00 => 4,
        0b01 => 5,
        _ => return Err(format!("reserved residual coding method {method:#04b}")),
    };
    let escape_code = (1u32 << parameter_bits) - 1;

    let partition_order = reader.read_bits(4)?;
    let partition_count = 1u32 << partition_order;
    // ブロックサイズはパーティション数で割り切れなければならない
    // (RFC 9639 Section 9.2.7)
    if !block_size.is_multiple_of(partition_count) {
        return Err(format!(
            "block size {block_size} is not divisible into {partition_count} partitions (RFC 9639 Section 9.2.7)"
        ));
    }
    let partition_samples = block_size >> partition_order;
    // 最初のパーティションのサンプル数は (block size >> partition order) -
    // predictor order であり、正でなければならない (RFC 9639 Section 9.2.7)
    if partition_samples <= predictor_order {
        return Err(format!(
            "partition sample count {partition_samples} does not exceed predictor order {predictor_order} (RFC 9639 Section 9.2.7)"
        ));
    }

    let mut partitions = Vec::with_capacity(partition_count as usize);
    for partition in 0..partition_count {
        let sample_count = if partition == 0 {
            partition_samples - predictor_order
        } else {
            partition_samples
        };
        let parameter = reader.read_bits(parameter_bits)?;
        if parameter == escape_code {
            // エスケープパーティション: 固定長の未符号化残差
            // (RFC 9639 Section 9.2.7.1)
            let bits = reader.read_bits(5)?;
            reader.skip(bits as usize * sample_count as usize)?;
            partitions.push(PartitionDiag::Escape(bits));
        } else {
            // Rice 符号 (RFC 9639 Section 9.2.7.2)。各サンプルは unary の
            // quotient + parameter bit の remainder。値を読まずに読み飛ばす
            for _ in 0..sample_count {
                reader.read_unary()?;
                reader.skip(parameter as usize)?;
            }
            partitions.push(PartitionDiag::Rice(parameter));
        }
    }

    Ok(ResidualDiag {
        parameter_bits: Some(parameter_bits),
        partition_order,
        partitions,
    })
}

/// 現在のサブフレームをフレームに確定する
///
/// フレームがまだ開始されていない場合はサブフレームを破棄する (本家の
/// 出力には現れないが、ヘッダー行より先にサブフレーム行が来た場合の防御)。
fn push_subframe(frame: &mut Option<FrameDiag>, subframe: &mut Option<SubframeDiag>) {
    if let Some(subframe) = subframe.take()
        && let Some(frame) = frame.as_mut()
    {
        frame.subframes.push(subframe);
    }
}

/// 現在のフレームを一覧に確定する
fn push_frame(frames: &mut Vec<FrameDiag>, frame: &mut Option<FrameDiag>) {
    if let Some(frame) = frame.take() {
        frames.push(frame);
    }
}

/// 現在のサブフレームとフレームをまとめて確定する
///
/// フレーム行の処理と EOF で同じ確定シーケンスが必要になるため、
/// 呼び出し漏れがないよう 1 関数にまとめる。
fn finish_frame(
    frames: &mut Vec<FrameDiag>,
    frame: &mut Option<FrameDiag>,
    subframe: &mut Option<SubframeDiag>,
) {
    push_subframe(frame, subframe);
    push_frame(frames, frame);
}

/// 本家 flac -a の分析出力 (.ana) をパースする
///
/// 出力形式は本家実装依存のため、未知のフィールド行は読み飛ばす。
/// 既知のフィールドに不正な値が現れた場合はエラーにする (入力の誤りを
/// 黙って取り込まないため)。
pub(crate) fn parse_reference_ana(text: &str) -> Result<Vec<FrameDiag>, String> {
    let mut frames: Vec<FrameDiag> = Vec::new();
    let mut current_frame: Option<FrameDiag> = None;
    let mut current_subframe: Option<SubframeDiag> = None;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('\t') {
            if let Some(rest) = rest.strip_prefix('\t') {
                // パーティションのパラメータ行 (例: parameter[0]=7)
                if let Some(value) = rest.strip_prefix("parameter[") {
                    let Some((_, value)) = value.split_once("]=") else {
                        continue;
                    };
                    let partition = if let Some(bits) = value.strip_prefix("ESCAPE, raw_bits=") {
                        PartitionDiag::Escape(bits.parse().map_err(|e| {
                            format!("failed to parse escape raw_bits {bits:?}: {e}")
                        })?)
                    } else {
                        PartitionDiag::Rice(value.parse().map_err(|e| {
                            format!("failed to parse Rice parameter {value:?}: {e}")
                        })?)
                    };
                    if let Some(SubframeDiag {
                        residual: Some(residual),
                        ..
                    }) = current_subframe.as_mut()
                    {
                        residual.partitions.push(partition);
                    }
                }
            } else {
                // サブフレーム行 (例: subframe=0 wasted_bits=0 type=CONSTANT value=0)
                let Some((_, rest)) = rest.split_once("subframe=") else {
                    continue;
                };
                let mut fields = rest.split('\t');
                let _index = fields.next();
                let mut wasted_bits = 0u32;
                let mut kind = SubframeKind::Verbatim;
                let mut type_seen = false;
                let mut has_residual = false;
                let mut parameter_bits = None;
                let mut partition_order = 0u32;
                for field in fields {
                    let Some((key, value)) = field.split_once('=') else {
                        continue;
                    };
                    match key {
                        "wasted_bits" => {
                            wasted_bits = value.parse().map_err(|e| {
                                format!("failed to parse wasted_bits {value:?}: {e}")
                            })?
                        }
                        "type" => {
                            type_seen = true;
                            match value {
                                "CONSTANT" => kind = SubframeKind::Constant,
                                "VERBATIM" => kind = SubframeKind::Verbatim,
                                // 次数・精度・シフトは後続のフィールドで上書きされる
                                "FIXED" => kind = SubframeKind::Fixed { order: 0 },
                                "LPC" => {
                                    kind = SubframeKind::Lpc {
                                        order: 0,
                                        precision: 0,
                                        shift: 0,
                                    }
                                }
                                _ => {
                                    return Err(format!("unknown subframe type {value:?}"));
                                }
                            }
                        }
                        "order" => {
                            let order = value
                                .parse()
                                .map_err(|e| format!("failed to parse order {value:?}: {e}"))?;
                            kind = match kind {
                                SubframeKind::Fixed { .. } => SubframeKind::Fixed { order },
                                SubframeKind::Lpc {
                                    precision, shift, ..
                                } => SubframeKind::Lpc {
                                    order,
                                    precision,
                                    shift,
                                },
                                _ => kind,
                            };
                        }
                        // LPC の係数精度 (RFC 9639 Section 9.2.6)
                        "qlp_coeff_precision" => {
                            let precision = value.parse().map_err(|e| {
                                format!("failed to parse qlp_coeff_precision {value:?}: {e}")
                            })?;
                            kind = match kind {
                                SubframeKind::Lpc { order, shift, .. } => SubframeKind::Lpc {
                                    order,
                                    precision,
                                    shift,
                                },
                                _ => kind,
                            };
                        }
                        // LPC の量子化シフト (符号付きで出力される)
                        "quantization_level" => {
                            let shift = value.parse().map_err(|e| {
                                format!("failed to parse quantization_level {value:?}: {e}")
                            })?;
                            kind = match kind {
                                SubframeKind::Lpc {
                                    order, precision, ..
                                } => SubframeKind::Lpc {
                                    order,
                                    precision,
                                    shift,
                                },
                                _ => kind,
                            };
                        }
                        "residual_type" => {
                            // 4 bit 方式は RICE、5 bit 方式は RICE2 と出力される
                            // (本家実装依存の表記。RFC 9639 Section 9.2.7 Table 23)
                            match value {
                                "RICE" => {
                                    parameter_bits = Some(4);
                                    has_residual = true;
                                }
                                "RICE2" => {
                                    parameter_bits = Some(5);
                                    has_residual = true;
                                }
                                _ => {
                                    return Err(format!("unsupported residual type {value:?}"));
                                }
                            }
                        }
                        "partition_order" => {
                            partition_order = value.parse().map_err(|e| {
                                format!("failed to parse partition_order {value:?}: {e}")
                            })?;
                        }
                        _ => {}
                    }
                }
                // 直前のサブフレームを確定してから新しいサブフレームを開始する
                push_subframe(&mut current_frame, &mut current_subframe);
                // サブフレーム行に type= が無いのは本家の出力形式からの逸脱。
                // 黙って VERBATIM として取り込むと差分が偽陽性になるためエラーにする
                if !type_seen {
                    return Err("subframe line without a type= field".to_string());
                }
                current_subframe = Some(SubframeDiag {
                    wasted_bits,
                    kind,
                    residual: if has_residual {
                        Some(ResidualDiag {
                            parameter_bits,
                            partition_order,
                            partitions: Vec::new(),
                        })
                    } else {
                        None
                    },
                });
            }
        } else if let Some(rest) = line.strip_prefix("frame=") {
            // フレーム行 (例: frame=0 offset=... bits=... blocksize=4096 ...)
            // 直前のフレームとサブフレームを確定する
            finish_frame(&mut frames, &mut current_frame, &mut current_subframe);
            let mut fields = rest.split('\t');
            let _index = fields.next();
            let mut block_size = 0u32;
            let mut sample_rate = 0u32;
            let mut channel_assignment = "";
            for field in fields {
                let Some((key, value)) = field.split_once('=') else {
                    continue;
                };
                match key {
                    "blocksize" => {
                        block_size = value
                            .parse()
                            .map_err(|e| format!("failed to parse blocksize {value:?}: {e}"))?
                    }
                    "sample_rate" => {
                        sample_rate = value
                            .parse()
                            .map_err(|e| format!("failed to parse sample_rate {value:?}: {e}"))?
                    }
                    "channel_assignment" => {
                        channel_assignment = match value {
                            "INDEPENDENT" => "INDEPENDENT",
                            "LEFT_SIDE" => "LEFT_SIDE",
                            "RIGHT_SIDE" => "RIGHT_SIDE",
                            "MID_SIDE" => "MID_SIDE",
                            _ => return Err(format!("unknown channel assignment {value:?}")),
                        };
                    }
                    _ => {}
                }
            }
            current_frame = Some(FrameDiag {
                block_size,
                sample_rate,
                channel_assignment,
                subframes: Vec::new(),
            });
        }
    }
    // 最後のフレームとサブフレームを確定する
    finish_frame(&mut frames, &mut current_frame, &mut current_subframe);
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本家 flac -a の出力例 (CONSTANT + FIXED / RICE、パーティション分割あり)
    const ANA_CONSTANT_FIXED: &str = "\
frame=0\toffset=8304\tbits=13280\tblocksize=4096\tsample_rate=44100\tchannels=2\tchannel_assignment=MID_SIDE
\tsubframe=0\twasted_bits=0\ttype=CONSTANT\tvalue=0
\tsubframe=1\twasted_bits=1\ttype=FIXED\torder=0\tresidual_type=RICE\tpartition_order=5
\t\tparameter[0]=0
\t\tparameter[1]=8
\t\tparameter[2]=ESCAPE, raw_bits=0
";

    /// 本家 flac -a の出力例 (LPC)
    const ANA_LPC: &str = "\
frame=0\toffset=8304\tbits=77280\tblocksize=4096\tsample_rate=44100\tchannels=2\tchannel_assignment=MID_SIDE
\tsubframe=0\twasted_bits=0\ttype=FIXED\torder=2\tresidual_type=RICE\tpartition_order=0
\t\twarmup[0]=-17188
\t\tparameter[0]=6
\tsubframe=1\twasted_bits=0\ttype=LPC\torder=3\tqlp_coeff_precision=12\tquantization_level=10\tresidual_type=RICE\tpartition_order=0
\t\tqlp_coeff[0]=1624
\t\twarmup[0]=-203
\t\tparameter[0]=6
";

    /// 本家 flac -a の出力例 (VERBATIM)
    const ANA_VERBATIM: &str = "\
frame=0\toffset=8304\tbits=131152\tblocksize=4096\tsample_rate=44100\tchannels=2\tchannel_assignment=INDEPENDENT
\tsubframe=0\twasted_bits=0\ttype=VERBATIM
\tsubframe=1\twasted_bits=0\ttype=VERBATIM
";

    #[test]
    fn parse_reference_ana_constant_fixed() {
        let frames = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.block_size, 4096);
        assert_eq!(frame.sample_rate, 44100);
        assert_eq!(frame.channel_assignment, "MID_SIDE");
        assert_eq!(frame.subframes.len(), 2);
        assert_eq!(frame.subframes[0].wasted_bits, 0);
        assert_eq!(frame.subframes[0].kind, SubframeKind::Constant);
        assert!(frame.subframes[0].residual.is_none());
        assert_eq!(frame.subframes[1].wasted_bits, 1);
        assert_eq!(frame.subframes[1].kind, SubframeKind::Fixed { order: 0 });
        let residual = frame.subframes[1]
            .residual
            .as_ref()
            .expect("FIXED サブフレームには残差があるはず");
        assert_eq!(residual.parameter_bits, Some(4));
        assert_eq!(residual.partition_order, 5);
        assert_eq!(
            residual.partitions,
            [
                PartitionDiag::Rice(0),
                PartitionDiag::Rice(8),
                PartitionDiag::Escape(0),
            ]
        );
    }

    #[test]
    fn parse_reference_ana_lpc() {
        let frames = parse_reference_ana(ANA_LPC).expect("パースに成功するはず");
        let frame = &frames[0];
        assert_eq!(frame.channel_assignment, "MID_SIDE");
        assert_eq!(frame.subframes[0].kind, SubframeKind::Fixed { order: 2 });
        assert_eq!(
            frame.subframes[1].kind,
            SubframeKind::Lpc {
                order: 3,
                precision: 12,
                shift: 10
            }
        );
        assert_eq!(
            frame.subframes[1]
                .residual
                .as_ref()
                .expect("残差があるはず")
                .partitions,
            [PartitionDiag::Rice(6)]
        );
    }

    #[test]
    fn parse_reference_ana_verbatim() {
        let frames = parse_reference_ana(ANA_VERBATIM).expect("パースに成功するはず");
        let frame = &frames[0];
        assert_eq!(frame.channel_assignment, "INDEPENDENT");
        assert_eq!(frame.subframes.len(), 2);
        assert!(frame.subframes[0].residual.is_none());
        assert_eq!(frame.subframes[0].kind, SubframeKind::Verbatim);
    }

    /// 本家 flac -a の出力例 (5 bit Rice 方式の RICE2)
    const ANA_RICE2: &str = "\
frame=0\toffset=8304\tbits=12345\tblocksize=4096\tsample_rate=44100\tchannels=2\tchannel_assignment=INDEPENDENT
\tsubframe=0\twasted_bits=0\ttype=LPC\torder=3\tqlp_coeff_precision=14\tquantization_level=12\tresidual_type=RICE2\tpartition_order=1
\t\tparameter[0]=7
\t\tparameter[1]=9
\tsubframe=1\twasted_bits=0\ttype=FIXED\torder=2\tresidual_type=RICE\tpartition_order=0
\t\tparameter[0]=6
";

    /// RICE2 (5 bit Rice) をパースでき、parameter_bits が記録される
    #[test]
    fn parse_reference_ana_rice2() {
        let frames = parse_reference_ana(ANA_RICE2).expect("パースに成功するはず");
        let frame = &frames[0];
        let residual = frame.subframes[0]
            .residual
            .as_ref()
            .expect("LPC サブフレームには残差があるはず");
        assert_eq!(residual.parameter_bits, Some(5));
        assert_eq!(
            residual.partitions,
            [PartitionDiag::Rice(7), PartitionDiag::Rice(9)]
        );
        let residual = frame.subframes[1]
            .residual
            .as_ref()
            .expect("FIXED サブフレームには残差があるはず");
        assert_eq!(residual.parameter_bits, Some(4));
    }

    /// 複数フレームをまたいだ .ana で、フレーム境界と最終確定が正しく行われる
    #[test]
    fn parse_reference_ana_multiple_frames() {
        let ana = "\
frame=0\toffset=8304\tbits=13280\tblocksize=4096\tsample_rate=44100\tchannels=2\tchannel_assignment=MID_SIDE
\tsubframe=0\twasted_bits=0\ttype=CONSTANT\tvalue=0
\tsubframe=1\twasted_bits=1\ttype=FIXED\torder=0\tresidual_type=RICE\tpartition_order=5
\t\tparameter[0]=0
\t\tparameter[1]=8
frame=1\toffset=2307\tbits=14080\tblocksize=3412\tsample_rate=44100\tchannels=2\tchannel_assignment=MID_SIDE
\tsubframe=0\twasted_bits=0\ttype=CONSTANT\tvalue=0
\tsubframe=1\twasted_bits=1\ttype=FIXED\torder=0\tresidual_type=RICE\tpartition_order=2
\t\tparameter[0]=7
\t\tparameter[1]=0
\t\tparameter[2]=0
\t\tparameter[3]=0
";
        let frames = parse_reference_ana(ana).expect("パースに成功するはず");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].block_size, 4096);
        assert_eq!(frames[0].subframes.len(), 2);
        assert_eq!(frames[1].block_size, 3412);
        let residual = frames[1].subframes[1]
            .residual
            .as_ref()
            .expect("残差があるはず");
        assert_eq!(residual.partition_order, 2);
        assert_eq!(residual.partitions.len(), 4);
    }

    /// 本家 flac -a の出力例 (フレーム末尾の最終フレーム確定を確認する 3 行目なしの最小例)
    #[test]
    fn parse_reference_ana_ends_without_newline() {
        let ana = "\
frame=0\toffset=8304\tbits=13280\tblocksize=4096\tsample_rate=44100\tchannels=2\tchannel_assignment=MID_SIDE
\tsubframe=0\twasted_bits=0\ttype=CONSTANT\tvalue=0";
        let frames = parse_reference_ana(ana).expect("パースに成功するはず");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].subframes.len(), 1);
    }

    /// エンコード済みのサンプル列 (テスト用ヘルパー)
    fn encode_silence(frames: usize) -> Vec<u8> {
        let samples: Vec<i32> = vec![0; frames * 2];
        let config = shiguredo_flac::encoder::StreamEncoderConfig {
            sample_rate: 44_100,
            channels: 2,
            bits_per_sample: 16,
            ..shiguredo_flac::encoder::StreamEncoderConfig::default()
        };
        shiguredo_flac::encoder::encode(config, &samples).expect("エンコードに成功するはず")
    }

    /// 無音信号 (CONSTANT) のビットストリームを診断できる
    #[test]
    fn analyze_parses_encoded_silence() {
        let data = encode_silence(4096);
        let frames = analyze(&data).expect("診断に成功するはず");
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.block_size, 4096);
        assert_eq!(frame.sample_rate, 44_100);
        assert_eq!(frame.channel_assignment, "INDEPENDENT");
        assert_eq!(frame.subframes.len(), 2);
        for subframe in &frame.subframes {
            assert_eq!(subframe.kind, SubframeKind::Constant);
            assert!(subframe.residual.is_none());
        }
    }

    /// 無音以外の信号でもフレーム数を正しく数えられる
    #[test]
    fn analyze_counts_frames() {
        // 4096 フレーム (インターリーブ 8192 サンプル) × 3 + 端数 100 フレーム
        let samples: Vec<i32> = (0..4096 * 3 * 2 + 200).map(|i| (i % 1000) - 500).collect();
        let config = shiguredo_flac::encoder::StreamEncoderConfig {
            sample_rate: 44_100,
            channels: 2,
            bits_per_sample: 16,
            ..shiguredo_flac::encoder::StreamEncoderConfig::default()
        };
        let data =
            shiguredo_flac::encoder::encode(config, &samples).expect("エンコードに成功するはず");
        let frames = analyze(&data).expect("診断に成功するはず");
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[3].block_size, 100);
    }

    /// 照合: パーティション数の不一致を検出する
    #[test]
    fn compare_detects_partition_count_mismatch() {
        let rs = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let mut reference = rs.clone();
        reference[0].subframes[1]
            .residual
            .as_mut()
            .expect("残差があるはず")
            .partitions
            .pop();
        let diffs = compare(&rs, &reference);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("partition count mismatch"));
    }

    /// 照合: Rice パラメータの方式 (4 bit / 5 bit) の不一致を検出する
    #[test]
    fn compare_detects_parameter_bits_mismatch() {
        let rs = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let mut reference = rs.clone();
        reference[0].subframes[1]
            .residual
            .as_mut()
            .expect("残差があるはず")
            .parameter_bits = Some(5);
        let diffs = compare(&rs, &reference);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("rice parameter bits mismatch"));
    }

    /// 照合: 残差の有無の不一致を検出する
    #[test]
    fn compare_detects_residual_presence_mismatch() {
        let rs = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let mut reference = rs.clone();
        reference[0].subframes[1].residual = None;
        let diffs = compare(&rs, &reference);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("flac-rs has a residual, reference does not"));
    }

    /// 照合: ブロックサイズの不一致を検出する
    #[test]
    fn compare_detects_block_size_mismatch() {
        let rs = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let mut reference = rs.clone();
        reference[0].block_size = 2048;
        let diffs = compare(&rs, &reference);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("block size mismatch"));
    }

    /// fLaC マーカーが無い入力はエラーになる
    #[test]
    fn analyze_rejects_non_flac() {
        let err = analyze(b"not a flac stream").expect_err("エラーになるはず");
        assert_eq!(err, "not a FLAC stream");
    }

    /// メタデータブロックが途中で切れた入力はエラーになる
    #[test]
    fn analyze_rejects_truncated_metadata() {
        let data = encode_silence(4096);
        let err = analyze(&data[..20]).expect_err("エラーになるはず");
        assert_eq!(err, "metadata block length exceeds the stream size");
    }

    /// フレームの同期コードが壊れているとエラーになる
    #[test]
    fn analyze_rejects_bad_frame_sync() {
        let mut data = encode_silence(4096);
        // 最初のフレームの先頭バイト (fLaC + STREAMINFO 42 バイト後) を壊す
        let frame_start = first_frame_offset(&data).expect("フレーム位置が取れるはず");
        data[frame_start] ^= 0xFF;
        let err = analyze(&data).expect_err("エラーになるはず");
        assert!(
            err.starts_with("frame sync mismatch"),
            "予期しないエラー: {err}"
        );
    }

    /// ビットリーダーの読み過ぎはエラーになる
    #[test]
    fn bit_reader_rejects_truncated_read() {
        let mut reader = BitReader::new(&[0b1010_1010]);
        assert_eq!(
            reader.read_bits(8).expect("8 bit は読めるはず"),
            0b1010_1010
        );
        assert!(reader.read_bit().is_err());
    }

    /// unary が終端に達しない入力はエラーになる
    #[test]
    fn bit_reader_rejects_unterminated_unary() {
        let mut reader = BitReader::new(&[0b0000_0000]);
        assert!(reader.read_unary().is_err());
    }

    /// 照合: 完全一致なら差分なし
    #[test]
    fn compare_identical_frames() {
        let frames = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        assert!(compare(&frames, &frames).is_empty());
    }

    /// 照合: フレーム数の不一致を検出する
    #[test]
    fn compare_detects_frame_count_mismatch() {
        let frames = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let diffs = compare(&frames, &[]);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("frame count mismatch"));
    }

    /// 照合: パーティションオーダーの不一致を検出する (個別パーティションの
    /// 比較は行わない)
    #[test]
    fn compare_detects_partition_order_mismatch() {
        let rs = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let mut reference = rs.clone();
        let residual = reference[0].subframes[1]
            .residual
            .as_mut()
            .expect("残差があるはず");
        residual.partition_order = 4;
        let diffs = compare(&rs, &reference);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("partition order mismatch"));
    }

    /// 照合: チャンネル割り当ての不一致を検出する
    #[test]
    fn compare_detects_channel_assignment_mismatch() {
        let rs = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let mut reference = rs.clone();
        reference[0].channel_assignment = "INDEPENDENT";
        let diffs = compare(&rs, &reference);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("channel assignment mismatch"));
    }

    /// 照合: wasted bits の不一致を検出する
    #[test]
    fn compare_detects_wasted_bits_mismatch() {
        let rs = parse_reference_ana(ANA_CONSTANT_FIXED).expect("パースに成功するはず");
        let mut reference = rs.clone();
        reference[0].subframes[1].wasted_bits = 0;
        let diffs = compare(&rs, &reference);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].contains("wasted bits mismatch"));
    }
}
