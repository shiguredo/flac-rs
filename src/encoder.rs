//! Sans I/O な FLAC ストリームエンコーダー (RFC 9639)
//!
//! `push_samples()` でチャンネルインターリーブ済みのサンプルを投入し、
//! `finish()` で完全な FLAC ストリームを得る。I/O は完全に利用者側の責務とする。
//!
//! ロスレス性を最優先とし、エンコード結果をデコードすると元のサンプル列と
//! 完全に一致することを保証する。
//!
//! ```rust
//! use shiguredo_flac::encoder::{StreamEncoder, StreamEncoderConfig};
//!
//! # fn main() -> Result<(), shiguredo_flac::error::EncodeError> {
//! let config = StreamEncoderConfig {
//!     sample_rate: 44100,
//!     channels: 2,
//!     bits_per_sample: 16,
//!     ..StreamEncoderConfig::default()
//! };
//! let mut encoder = StreamEncoder::new(config)?;
//! // インターリーブ済み (L, R, L, R, ...) のサンプルを投入する
//! encoder.push_samples(&[100, -100, 200, -200, 300, -300])?;
//! let flac_bytes = encoder.finish()?;
//! assert_eq!(&flac_bytes[..4], b"fLaC");
//! # Ok(())
//! # }
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::bit_writer::BitWriter;
use crate::crc::crc16;
use crate::decoder::STREAM_MARKER;
use crate::error::EncodeError;
use crate::frame::{BlockingStrategy, ChannelAssignment, FrameHeader};
use crate::md5::Md5;
use crate::metadata::{MetadataBlock, StreamInfo};
use crate::subframe::{PlanScratch, SubframeOptions, SubframePlan};

/// LPC 係数の量子化精度 (ビット数)
///
/// フォーマット上の上限は 15 bit (RFC 9639 Section 9.2.6)。リファレンス実装が
/// 標準的なブロックサイズで使う 14 bit を採用する。
const LPC_PRECISION: u32 = 14;

/// エンコーダーの設定
#[derive(Debug, Clone)]
pub struct StreamEncoderConfig {
    /// サンプルレート (Hz)。1-1048575 (RFC 9639 Section 8.2)
    pub sample_rate: u32,
    /// チャンネル数 (1-8)
    pub channels: u8,
    /// サンプルあたりのビット数 (4-32)
    pub bits_per_sample: u8,
    /// ブロックサイズ (インターチャンネルサンプル数)。16-65535
    pub block_size: u16,
    /// LPC の最大次数 (0-32)。0 なら固定予測のみ使う
    pub max_lpc_order: u8,
    /// Rice パーティションの最大オーダー (0-15)
    pub max_partition_order: u8,
    /// ステレオデコリレーション (mid-side 等) を試すか (2 チャンネル時のみ有効)
    pub stereo_decorrelation: bool,
    /// STREAMINFO 以外の追加メタデータブロック
    pub metadata: Vec<MetadataBlock>,
}

impl Default for StreamEncoderConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            channels: 2,
            bits_per_sample: 16,
            block_size: 4096,
            max_lpc_order: 8,
            max_partition_order: 4,
            stereo_decorrelation: true,
            metadata: Vec::new(),
        }
    }
}

/// Sans I/O な FLAC ストリームエンコーダー
///
/// STREAMINFO の合計サンプル数・MD5・フレームサイズ統計はエンコード完了時に
/// 確定するため、出力は `finish()` でまとめて返す。
#[derive(Debug)]
pub struct StreamEncoder {
    config: StreamEncoderConfig,
    /// 出力バッファ (fLaC マーカー + メタデータ + フレーム列)
    out: Vec<u8>,
    /// ブロックサイズ未満の未エンコードサンプル (インターリーブ済み)
    pending: Vec<i32>,
    /// エンコード前サンプルの MD5 (RFC 9639 Section 8.2)
    md5: Md5,
    /// MD5 計算用の作業バッファ (フレームごとの再確保を避けるため再利用する)
    md5_buf: Vec<u8>,
    /// エンコード済みのインターチャンネルサンプル数
    samples_encoded: u64,
    /// エンコード済みのフレーム数
    frames_encoded: u64,
    /// フレームサイズの実測最小値 (バイト)。フレームがなければ None
    min_frame_size: Option<u32>,
    /// フレームサイズの実測最大値 (バイト)。フレームがなければ None
    max_frame_size: Option<u32>,
    /// サブフレーム計画の作業バッファ (フレームごとの再確保を避けるため再利用する)
    scratch: PlanScratch,
    /// チャンネル分割・ステレオデコリレーション用の i64 バッファプール
    /// (フレームごとの再確保を避けるため再利用する)
    channel_bufs: Vec<Vec<i64>>,
}

impl StreamEncoder {
    /// エンコーダーを作成し、fLaC マーカーとメタデータブロックを出力バッファに
    /// 書き込む
    pub fn new(config: StreamEncoderConfig) -> Result<Self, EncodeError> {
        // 設定の検証 (RFC 9639 Section 8.2)
        if config.sample_rate == 0 || config.sample_rate > 0xF_FFFF {
            return Err(EncodeError::InvalidConfig(format!(
                "sample rate must be 1-1048575, got {} (RFC 9639 Section 8.2)",
                config.sample_rate
            )));
        }
        if !(1..=8).contains(&config.channels) {
            return Err(EncodeError::InvalidConfig(format!(
                "channels must be 1-8, got {} (RFC 9639 Section 8.2)",
                config.channels
            )));
        }
        if !(4..=32).contains(&config.bits_per_sample) {
            return Err(EncodeError::InvalidConfig(format!(
                "bits per sample must be 4-32, got {} (RFC 9639 Section 8.2)",
                config.bits_per_sample
            )));
        }
        // u16 型のため 65535 以下は型システムで保証される
        if config.block_size < 16 {
            return Err(EncodeError::InvalidConfig(format!(
                "block size must be 16-65535, got {} (RFC 9639 Section 8.2)",
                config.block_size
            )));
        }
        if config.max_lpc_order > 32 {
            return Err(EncodeError::InvalidConfig(format!(
                "max LPC order must be 0-32, got {} (RFC 9639 Section 9.2.6)",
                config.max_lpc_order
            )));
        }
        // RFC 9639 Section 7 (Streamable Subset) では max_partition_order <= 8 だが、
        // 本エンコーダーは 9-15 も非ストリーミング用途として許容する
        if config.max_partition_order > 15 {
            return Err(EncodeError::InvalidConfig(format!(
                "max partition order must be 0-15, got {} (RFC 9639 Section 9.2.7)",
                config.max_partition_order
            )));
        }
        for block in &config.metadata {
            if matches!(block, MetadataBlock::StreamInfo(_)) {
                return Err(EncodeError::InvalidMetadata(String::from(
                    "streaminfo is generated by the encoder and must not be supplied (RFC 9639 Section 8.2)",
                )));
            }
        }
        // シークテーブルと Vorbis コメントはストリームに 1 つまで
        // (RFC 9639 Section 8.5, 8.6)
        let seek_tables = config
            .metadata
            .iter()
            .filter(|b| matches!(b, MetadataBlock::SeekTable(_)))
            .count();
        if seek_tables > 1 {
            return Err(EncodeError::InvalidMetadata(String::from(
                "at most one seek table metadata block is allowed (RFC 9639 Section 8.5)",
            )));
        }
        let vorbis_comments = config
            .metadata
            .iter()
            .filter(|b| matches!(b, MetadataBlock::VorbisComment(_)))
            .count();
        if vorbis_comments > 1 {
            return Err(EncodeError::InvalidMetadata(String::from(
                "at most one vorbis comment metadata block is allowed (RFC 9639 Section 8.6)",
            )));
        }

        // fLaC マーカーと STREAMINFO (暫定値) を書く。合計サンプル数・MD5・
        // フレームサイズは finish() で確定するまで 0 (不明) とする
        let mut out = Vec::new();
        out.extend_from_slice(&STREAM_MARKER);
        let placeholder = StreamInfo {
            min_block_size: config.block_size,
            max_block_size: config.block_size,
            min_frame_size: 0,
            max_frame_size: 0,
            sample_rate: config.sample_rate,
            channels: config.channels,
            bits_per_sample: config.bits_per_sample,
            total_samples: 0,
            md5: [0u8; 16],
        };
        let streaminfo_block = MetadataBlock::StreamInfo(placeholder);
        out.extend_from_slice(&streaminfo_block.encode(config.metadata.is_empty())?);
        for (i, block) in config.metadata.iter().enumerate() {
            let is_last = i + 1 == config.metadata.len();
            out.extend_from_slice(&block.encode(is_last)?);
        }

        Ok(Self {
            config,
            out,
            pending: Vec::new(),
            md5: Md5::new(),
            md5_buf: Vec::new(),
            samples_encoded: 0,
            frames_encoded: 0,
            min_frame_size: None,
            scratch: PlanScratch::default(),
            channel_bufs: Vec::new(),
            max_frame_size: None,
        })
    }

    /// チャンネルインターリーブ済みのサンプルを投入する
    ///
    /// サンプル数はチャンネル数の倍数でなければならない。ブロックサイズ分の
    /// サンプルが揃うたびに内部でフレームにエンコードされる。
    pub fn push_samples(&mut self, interleaved: &[i32]) -> Result<(), EncodeError> {
        if !interleaved
            .len()
            .is_multiple_of(usize::from(self.config.channels))
        {
            return Err(EncodeError::UnalignedSamples {
                count: interleaved.len(),
                channels: self.config.channels,
            });
        }
        // サンプル値がビット深度に収まることを検証する。範囲チェックは
        // 分岐のない蓄積で行い (自動ベクトル化される)、逸脱があった場合
        // だけ走査し直して具体値を報告する
        let low = -(1i64 << (self.config.bits_per_sample - 1));
        let high = (1i64 << (self.config.bits_per_sample - 1)) - 1;
        let mut all_in_range = true;
        for &sample in interleaved {
            // low <= sample <= high は減算 1 回 + 符号なし比較 1 回と同値
            all_in_range &= (i64::from(sample).wrapping_sub(low) as u64) <= (high - low) as u64;
        }
        if !all_in_range {
            let value = interleaved
                .iter()
                .copied()
                .find(|&sample| i64::from(sample) < low || i64::from(sample) > high)
                .expect("範囲逸脱を検出済み (実装バグ)");
            return Err(EncodeError::SampleOutOfRange {
                value,
                bits_per_sample: self.config.bits_per_sample,
            });
        }
        // 合計サンプル数は STREAMINFO の 36 bit に収まらなければならない
        // (RFC 9639 Section 8.2)
        let added = (interleaved.len() / usize::from(self.config.channels)) as u64;
        let total = self
            .samples_encoded
            .checked_add(self.pending.len() as u64 / u64::from(self.config.channels))
            .and_then(|n| n.checked_add(added))
            .ok_or(EncodeError::TooManySamples)?;
        if total > 0xF_FFFF_FFFF {
            return Err(EncodeError::TooManySamples);
        }

        // ブロックサイズ分揃うたびにフレームへエンコードする。端数バッファを
        // 経由するのはブロックに満たない分だけにし、残りは入力スライスから
        // 直接エンコードしてコピーと drain による残データの移動を避ける
        let block_samples = usize::from(self.config.block_size) * usize::from(self.config.channels);
        let mut rest = interleaved;
        if !self.pending.is_empty() {
            let take = (block_samples - self.pending.len()).min(rest.len());
            self.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.pending.len() == block_samples {
                // 端数バッファを取り出してエンコードし、空にして戻す
                // (確保済みの容量を再利用する)
                let block = core::mem::take(&mut self.pending);
                self.encode_frame(&block)?;
                self.pending = block;
                self.pending.clear();
            }
        }
        while rest.len() >= block_samples {
            self.encode_frame(&rest[..block_samples])?;
            rest = &rest[block_samples..];
        }
        self.pending.extend_from_slice(rest);
        Ok(())
    }

    /// 残りのサンプルを最終フレームとしてエンコードし、STREAMINFO を確定して
    /// 完全な FLAC ストリームを返す
    pub fn finish(mut self) -> Result<Vec<u8>, EncodeError> {
        if !self.pending.is_empty() {
            // 最終フレームはブロックサイズ未満でもよい (RFC 9639 Section 8.2)
            let block: Vec<i32> = core::mem::take(&mut self.pending);
            self.encode_frame(&block)?;
        }
        // STREAMINFO のペイロードを確定値で書き直す。
        // ペイロードは fLaC (4 バイト) + ブロックヘッダー (4 バイト) の直後にある。
        const STREAMINFO_OFFSET: usize = 8;
        const STREAMINFO_PAYLOAD_LEN: usize = 34;
        let streaminfo = StreamInfo {
            min_block_size: self.config.block_size,
            max_block_size: self.config.block_size,
            min_frame_size: self.min_frame_size.unwrap_or(0),
            max_frame_size: self.max_frame_size.unwrap_or(0),
            sample_rate: self.config.sample_rate,
            channels: self.config.channels,
            bits_per_sample: self.config.bits_per_sample,
            total_samples: self.samples_encoded,
            md5: self.md5.finalize(),
        };
        let payload = streaminfo.encode_payload()?;
        self.out[STREAMINFO_OFFSET..STREAMINFO_OFFSET + STREAMINFO_PAYLOAD_LEN]
            .copy_from_slice(&payload);
        Ok(self.out)
    }

    /// 1 ブロック分のインターリーブ済みサンプルをフレームにエンコードする
    fn encode_frame(&mut self, interleaved: &[i32]) -> Result<(), EncodeError> {
        let channels = usize::from(self.config.channels);
        let block_size = (interleaved.len() / channels) as u16;
        let bits = u32::from(self.config.bits_per_sample);

        // MD5 はエンコード前のサンプルに対して計算する
        // (インターリーブ順・signed little-endian・バイト整列)
        // (RFC 9639 Section 8.2)
        let bytes_per_sample = usize::from(self.config.bits_per_sample).div_ceil(8);
        crate::md5::samples_to_md5_bytes(interleaved, bytes_per_sample, &mut self.md5_buf);
        self.md5.update(&self.md5_buf);

        // チャンネルごとのサンプル列に分ける。バッファはプールから取り出して
        // 使い回す (フレームごとの malloc を避ける)
        let mut channel_samples: Vec<Vec<i64>> = Vec::with_capacity(channels);
        if channels == 2 {
            // 2 チャンネル (最も一般的) はペア読み出しに特殊化する。
            // chunks_exact はストライド走査より自動ベクトル化されやすい
            let mut left = self.channel_bufs.pop().unwrap_or_default();
            left.clear();
            left.extend(interleaved.chunks_exact(2).map(|pair| i64::from(pair[0])));
            let mut right = self.channel_bufs.pop().unwrap_or_default();
            right.clear();
            right.extend(interleaved.chunks_exact(2).map(|pair| i64::from(pair[1])));
            channel_samples.push(left);
            channel_samples.push(right);
        } else {
            for channel in 0..channels {
                let mut samples = self.channel_bufs.pop().unwrap_or_default();
                samples.clear();
                samples.extend(
                    interleaved[channel..]
                        .iter()
                        .step_by(channels)
                        .map(|&sample| i64::from(sample)),
                );
                channel_samples.push(samples);
            }
        }

        let options = SubframeOptions {
            max_lpc_order: usize::from(self.config.max_lpc_order),
            lpc_precision: LPC_PRECISION,
            max_partition_order: u32::from(self.config.max_partition_order),
        };

        // チャンネル割り当てを決める (RFC 9639 Section 4.2)
        let (assignment, plans) = Self::plan_channels(
            &self.config,
            &channel_samples,
            bits,
            &options,
            &mut self.scratch,
            &mut self.channel_bufs,
        );

        // フレームを出力バッファの続きへ直接書き出す (中間バッファを避ける)
        let header = FrameHeader {
            blocking_strategy: BlockingStrategy::Fixed,
            block_size,
            sample_rate: Some(self.config.sample_rate),
            channel_assignment: assignment,
            bits_per_sample: Some(self.config.bits_per_sample),
            coded_number: self.frames_encoded,
        };
        let frame_start = self.out.len();
        let mut writer = BitWriter::resume(core::mem::take(&mut self.out));
        let result = Self::write_frame(&mut writer, &header, &plans);
        self.out = writer.into_bytes();
        // 書き出しが終わった計画とチャンネルバッファを回収して使い回す
        for plan in plans {
            plan.recycle(&mut self.scratch);
        }
        for buf in channel_samples {
            self.channel_bufs.push(buf);
        }
        if result.is_err() {
            // 書きかけのフレームを出力バッファに残さない
            self.out.truncate(frame_start);
        }
        result?;

        // フレームサイズの統計を更新する (STREAMINFO 用)。24 bit を超える場合は
        // 不明 (0) のままにする (RFC 9639 Section 8.2)
        if let Ok(size) = u32::try_from(self.out.len() - frame_start)
            && size <= 0xFF_FFFF
        {
            self.min_frame_size = Some(self.min_frame_size.map_or(size, |v| v.min(size)));
            self.max_frame_size = Some(self.max_frame_size.map_or(size, |v| v.max(size)));
        }

        self.samples_encoded += u64::from(block_size);
        self.frames_encoded += 1;
        Ok(())
    }

    /// フレーム本体 (ヘッダー + サブフレーム + CRC-16) を書き出す
    fn write_frame(
        writer: &mut BitWriter,
        header: &FrameHeader,
        plans: &[SubframePlan],
    ) -> Result<(), EncodeError> {
        let frame_start = writer.byte_len();
        header.encode(writer)?;
        for plan in plans {
            plan.encode(writer);
        }
        // バイト境界まで 0 詰めして CRC-16 (RFC 9639 Section 9.3)
        writer.align_to_byte();
        let crc = crc16(&writer.as_bytes()[frame_start..]);
        writer.write_u32(u32::from(crc), 16);
        Ok(())
    }

    /// チャンネル割り当てと各サブフレームの計画を決める
    ///
    /// 2 チャンネルでステレオデコリレーションが有効な場合は 4 モード
    /// (independent / left-side / side-right / mid-side) を全て計画し、
    /// 合計ビット数が最小のものを選ぶ (RFC 9639 Section 4.2)。
    fn plan_channels(
        config: &StreamEncoderConfig,
        channel_samples: &[Vec<i64>],
        bits: u32,
        options: &SubframeOptions,
        scratch: &mut PlanScratch,
        channel_bufs: &mut Vec<Vec<i64>>,
    ) -> (ChannelAssignment, Vec<SubframePlan>) {
        let channels = channel_samples.len() as u8;
        if channels != 2 || !config.stereo_decorrelation {
            let plans: Vec<SubframePlan> = channel_samples
                .iter()
                .map(|samples| SubframePlan::new(samples, bits, options, scratch))
                .collect();
            return (ChannelAssignment::Independent(channels), plans);
        }

        let left = &channel_samples[0];
        let right = &channel_samples[1];
        // mid = (left + right) >> 1、side = left - right (RFC 9639 Section 4.2)。
        // 全て i64 で計算するためオーバーフローしない (RFC 9639 Appendix A.2)。
        // バッファはプールから取り出して使い回す
        let mut mid = channel_bufs.pop().unwrap_or_default();
        mid.clear();
        mid.extend(left.iter().zip(right.iter()).map(|(&l, &r)| (l + r) >> 1));
        let mut side = channel_bufs.pop().unwrap_or_default();
        side.clear();
        side.extend(left.iter().zip(right.iter()).map(|(&l, &r)| l - r));

        let left_plan = SubframePlan::new(left, bits, options, scratch);
        let right_plan = SubframePlan::new(right, bits, options, scratch);
        // サイドチャンネルはビット深度が 1 bit 増える (RFC 9639 Section 4.2)
        let side_plan = SubframePlan::new(&side, bits + 1, options, scratch);
        let mid_plan = SubframePlan::new(&mid, bits, options, scratch);
        // mid / side は計画がサンプルを所有コピーするため、ここで返却できる
        channel_bufs.push(mid);
        channel_bufs.push(side);

        let independent = left_plan.bits() + right_plan.bits();
        let left_side = left_plan.bits() + side_plan.bits();
        let side_right = side_plan.bits() + right_plan.bits();
        let mid_side = mid_plan.bits() + side_plan.bits();

        let best = independent.min(left_side).min(side_right).min(mid_side);
        if best == independent {
            (
                ChannelAssignment::Independent(2),
                alloc::vec![left_plan, right_plan],
            )
        } else if best == mid_side {
            (ChannelAssignment::MidSide, alloc::vec![mid_plan, side_plan])
        } else if best == left_side {
            (
                ChannelAssignment::LeftSide,
                alloc::vec![left_plan, side_plan],
            )
        } else {
            (
                ChannelAssignment::SideRight,
                alloc::vec![side_plan, right_plan],
            )
        }
    }
}

/// サンプル列全体を一括で FLAC ストリームにエンコードする
///
/// ストリーミングが不要な場合の簡便 API。
pub fn encode(config: StreamEncoderConfig, interleaved: &[i32]) -> Result<Vec<u8>, EncodeError> {
    let mut encoder = StreamEncoder::new(config)?;
    encoder.push_samples(interleaved)?;
    encoder.finish()
}
