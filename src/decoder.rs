//! Sans I/O な FLAC ストリームデコーダー (RFC 9639)
//!
//! `feed()` でバイト列を投入し、`decode_frame()` でフレームを 1 つずつ取り出す。
//! I/O は完全に利用者側の責務とする。
//!
//! ```rust
//! use shiguredo_flac::decoder::StreamDecoder;
//!
//! # fn main() -> Result<(), shiguredo_flac::error::DecodeError> {
//! # let flac_bytes: &[u8] = &[
//! #     0x66, 0x4c, 0x61, 0x43, 0x80, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00,
//! #     0x00, 0x00, 0x0f, 0x00, 0x00, 0x0f, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00,
//! #     0x00, 0x01, 0x3e, 0x84, 0xb4, 0x18, 0x07, 0xdc, 0x69, 0x03, 0x07, 0x58,
//! #     0x6a, 0x3d, 0xad, 0x1a, 0x2e, 0x0f, 0xff, 0xf8, 0x69, 0x18, 0x00, 0x00,
//! #     0xbf, 0x03, 0x58, 0xfd, 0x03, 0x12, 0x8b, 0xaa, 0x9a,
//! # ];
//! let mut decoder = StreamDecoder::new();
//! decoder.feed(flac_bytes);
//! decoder.finish();
//!
//! while let Some(frame) = decoder.decode_frame()? {
//!     // frame.samples はチャンネルインターリーブ済みのサンプル列
//!     assert_eq!(frame.channels, 2);
//! }
//! # Ok(())
//! # }
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::bit_reader::BitReader;
use crate::crc::crc16;
use crate::error::{DecodeError, ParseError};
use crate::frame::{BlockingStrategy, ChannelAssignment, FrameHeader};
use crate::md5::Md5;
use crate::metadata::{MetadataBlock, StreamInfo};
use crate::subframe::decode_subframe;

/// fLaC ストリームマーカー (RFC 9639 Section 6)
pub(crate) const STREAM_MARKER: [u8; 4] = *b"fLaC";

/// 1 フレームの符号化サイズの上限 (バイト)
///
/// 合法なフレームの現実的な上限 (VERBATIM 33 bit x 65535 サンプル x 8 チャンネル
/// で約 2.2 MB) を大きく超える値。壊れた・悪意ある入力によるメモリ浪費を防ぐ
/// ための防衛的な制限で、RFC 9639 にこのような制限はない。
const MAX_FRAME_BYTES: usize = 1 << 26;

/// デコードした 1 フレーム分の音声
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    /// フレームヘッダー
    pub header: FrameHeader,
    /// チャンネルインターリーブ済みのサンプル列
    /// (長さ = ブロックサイズ x チャンネル数)
    pub samples: Vec<i32>,
    /// このフレームの最初のインターチャンネルサンプル番号
    pub first_sample_number: u64,
    /// 実効サンプルレート (Hz)。フレームヘッダーまたは STREAMINFO 由来
    pub sample_rate: u32,
    /// 実効ビット深度
    pub bits_per_sample: u8,
    /// チャンネル数
    pub channels: u8,
}

/// デコーダーの進行フェーズ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// fLaC ストリームマーカー待ち
    Marker,
    /// メタデータブロック待ち
    Metadata,
    /// オーディオフレーム待ち
    Frames,
}

/// Sans I/O な FLAC ストリームデコーダー
///
/// 入力の終端では `finish()` を呼ぶこと。終端通知により、途中で切れた
/// ストリームの検出と MD5 チェックサムの検証が行われる。
#[derive(Debug)]
pub struct StreamDecoder {
    /// 入力データのバッファ (先頭 `consumed` バイトは処理済み)
    buf: Vec<u8>,
    /// `buf` の先頭からの処理済みバイト数
    ///
    /// 処理のたびに `Vec::drain` で先頭を詰めるとフレームごとに残データの
    /// 移動 (memmove) が発生するため、オフセットで管理して `compact()` で
    /// まとめて詰める。
    consumed: usize,
    phase: Phase,
    /// `finish()` が呼ばれたか
    finished: bool,
    stream_info: Option<StreamInfo>,
    metadata: Vec<MetadataBlock>,
    /// デコード済みサンプルの MD5 (STREAMINFO の値と照合する)
    md5: Md5,
    /// MD5 計算用の作業バッファ (フレームごとの再確保を避けるため再利用する)
    md5_buf: Vec<u8>,
    /// サブフレームデコードのチャンネル別作業バッファ
    /// (フレームごとの再確保を避けるため再利用する)
    channel_buf: Vec<Vec<i64>>,
    /// ストリーム終端の検証を済ませたか
    end_checked: bool,
    /// デコード済みのインターチャンネルサンプル数
    samples_decoded: u64,
    /// デコード済みのフレーム数
    frames_decoded: u64,
    /// 最初のフレームのブロッキング戦略 (以降のフレームで変わってはならない)
    blocking_strategy: Option<BlockingStrategy>,
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            consumed: 0,
            phase: Phase::Marker,
            finished: false,
            stream_info: None,
            metadata: Vec::new(),
            md5: Md5::new(),
            md5_buf: Vec::new(),
            channel_buf: Vec::new(),
            end_checked: false,
            samples_decoded: 0,
            frames_decoded: 0,
            blocking_strategy: None,
        }
    }

    /// 入力データを投入する
    ///
    /// `finish()` 呼び出し後は無視される。
    pub fn feed(&mut self, data: &[u8]) {
        if self.finished {
            return;
        }
        self.buf.extend_from_slice(data);
    }

    /// 未処理のデータ
    fn pending(&self) -> &[u8] {
        &self.buf[self.consumed..]
    }

    /// 先頭 `len` バイトを処理済みにする
    ///
    /// 処理済み領域が十分大きくなったときだけまとめて解放し、
    /// フレームごとの memmove を避ける。
    fn advance(&mut self, len: usize) {
        self.consumed += len;
        if self.consumed >= 4096 && self.consumed * 2 >= self.buf.len() {
            self.buf.drain(..self.consumed);
            self.consumed = 0;
        }
    }

    /// 入力の終端を通知する
    ///
    /// これ以降の `feed()` は無視される。`decode_frame()` が残りのフレームを
    /// 返し終えると、ストリームの完全性 (MD5 / 総サンプル数) が検証される。
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// STREAMINFO (デコード済みであれば)
    pub fn stream_info(&self) -> Option<&StreamInfo> {
        self.stream_info.as_ref()
    }

    /// デコード済みのメタデータブロック列 (STREAMINFO 含む)
    pub fn metadata(&self) -> &[MetadataBlock] {
        &self.metadata
    }

    /// フレームを 1 つデコードする
    ///
    /// - `Ok(Some(frame))`: フレームをデコードした
    /// - `Ok(None)`: データ不足 (追加の `feed()` 待ち)、または `finish()` 済みで
    ///   ストリーム終端に達した
    /// - `Err(_)`: フォーマット違反・チェックサム不一致
    pub fn decode_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        loop {
            match self.phase {
                Phase::Marker => {
                    let pending = self.pending();
                    if pending.len() < 4 {
                        return self.need_more_data();
                    }
                    if pending[..4] != STREAM_MARKER {
                        let mut found = [0u8; 4];
                        found.copy_from_slice(&pending[..4]);
                        return Err(DecodeError::InvalidStreamMarker { found });
                    }
                    self.advance(4);
                    self.phase = Phase::Metadata;
                }
                Phase::Metadata => {
                    // メタデータブロックヘッダー (RFC 9639 Section 8.1)
                    let pending = self.pending();
                    if pending.len() < 4 {
                        return self.need_more_data();
                    }
                    let is_last = pending[0] & 0x80 != 0;
                    let block_type = pending[0] & 0x7F;
                    let size = usize::from(pending[1]) << 16
                        | usize::from(pending[2]) << 8
                        | usize::from(pending[3]);
                    if pending.len() < 4 + size {
                        return self.need_more_data();
                    }
                    let block = MetadataBlock::decode(block_type, &pending[4..4 + size])?;
                    self.advance(4 + size);

                    // 最初のメタデータブロックは STREAMINFO でなければならない
                    // (RFC 9639 Section 8)
                    match &block {
                        MetadataBlock::StreamInfo(info) => {
                            if self.stream_info.is_some() {
                                return Err(DecodeError::InvalidData(String::from(
                                    "duplicate streaminfo metadata block (RFC 9639 Section 8.2)",
                                )));
                            }
                            self.stream_info = Some(info.clone());
                        }
                        _ if self.stream_info.is_none() => {
                            return Err(DecodeError::InvalidData(String::from(
                                "first metadata block must be streaminfo (RFC 9639 Section 8)",
                            )));
                        }
                        // シークテーブルはストリームに 1 つまで (RFC 9639 Section 8.5)
                        MetadataBlock::SeekTable(_)
                            if self
                                .metadata
                                .iter()
                                .any(|b| matches!(b, MetadataBlock::SeekTable(_))) =>
                        {
                            return Err(DecodeError::InvalidData(String::from(
                                "duplicate seek table metadata block (RFC 9639 Section 8.5)",
                            )));
                        }
                        // Vorbis コメントはストリームに 1 つまで (RFC 9639 Section 8.6)
                        MetadataBlock::VorbisComment(_)
                            if self
                                .metadata
                                .iter()
                                .any(|b| matches!(b, MetadataBlock::VorbisComment(_))) =>
                        {
                            return Err(DecodeError::InvalidData(String::from(
                                "duplicate vorbis comment metadata block (RFC 9639 Section 8.6)",
                            )));
                        }
                        _ => {}
                    }
                    self.metadata.push(block);
                    if is_last {
                        self.phase = Phase::Frames;
                    }
                }
                Phase::Frames => {
                    if self.pending().is_empty() {
                        return self.need_more_data();
                    }
                    // 作業バッファはいったん取り出して渡す (バッファへの可変
                    // 借用と入力バッファへの不変借用が衝突するため)。
                    // どの結果でも取り出したバッファを戻して容量を再利用する
                    let mut channel_buf = core::mem::take(&mut self.channel_buf);
                    let result = self.parse_frame(&mut channel_buf);
                    self.channel_buf = channel_buf;
                    return match result {
                        Ok((frame, consumed)) => {
                            self.advance(consumed);
                            self.update_md5(&frame);
                            self.samples_decoded += u64::from(frame.header.block_size);
                            self.frames_decoded += 1;
                            Ok(Some(frame))
                        }
                        Err(ParseError::NeedMoreData) => {
                            if self.finished {
                                // 入力終端なのにフレームが完結しない
                                Err(DecodeError::TruncatedStream)
                            } else if self.pending().len() > MAX_FRAME_BYTES {
                                Err(DecodeError::InvalidData(format!(
                                    "frame exceeds the maximum supported size of {} bytes",
                                    MAX_FRAME_BYTES
                                )))
                            } else {
                                Ok(None)
                            }
                        }
                        Err(ParseError::Invalid(e)) => Err(e),
                    };
                }
            }
        }
    }

    /// データ不足時の共通処理
    ///
    /// `finish()` 済みなら途中で切れたストリームとして扱う。ただしフレーム
    /// フェーズでバッファが空なら正常なストリーム終端で、MD5 と総サンプル数を
    /// 検証する。
    fn need_more_data(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        if !self.finished {
            return Ok(None);
        }
        if self.phase == Phase::Frames && self.pending().is_empty() {
            self.verify_end()?;
            return Ok(None);
        }
        Err(DecodeError::TruncatedStream)
    }

    /// ストリーム終端の検証 (総サンプル数と MD5) (RFC 9639 Section 8.2)
    fn verify_end(&mut self) -> Result<(), DecodeError> {
        if self.end_checked {
            return Ok(());
        }
        self.end_checked = true;
        let info = self
            .stream_info
            .as_ref()
            .expect("フレームフェーズでは STREAMINFO が存在する (実装バグ)");
        // 総サンプル数 0 は不明を表す (RFC 9639 Section 8.2)
        if info.total_samples != 0 && info.total_samples != self.samples_decoded {
            return Err(DecodeError::InvalidData(format!(
                "stream has {} samples but streaminfo claims {} (RFC 9639 Section 8.2)",
                self.samples_decoded, info.total_samples
            )));
        }
        // MD5 が全て 0 は不明を表す (RFC 9639 Section 8.2)
        if info.md5 != [0u8; 16] {
            // verify_end は end_checked で高々1回しか呼ばれない。
            // Md5 は Default 未実装のため take() は使えず、clone の
            // ヒープコピーはストリーム終端の 1 回限りなので許容する。
            let actual = self.md5.clone().finalize();
            if actual != info.md5 {
                return Err(DecodeError::Md5Mismatch {
                    expected: info.md5,
                    actual,
                });
            }
        }
        Ok(())
    }

    /// デコード済みサンプルを MD5 計算に反映する
    ///
    /// サンプルはインターリーブ順・signed little-endian・バイト整列で
    /// 並べる (RFC 9639 Section 8.2)。
    fn update_md5(&mut self, frame: &DecodedFrame) {
        let bytes_per_sample = usize::from(frame.bits_per_sample).div_ceil(8);
        crate::md5::samples_to_md5_bytes(&frame.samples, bytes_per_sample, &mut self.md5_buf);
        self.md5.update(&self.md5_buf);
    }

    /// バッファ先頭からフレームを 1 つパースする
    ///
    /// 成功時は (フレーム, 消費バイト数) を返す。`channel_samples` は
    /// チャンネル別サンプルの作業バッファで、呼び出しごとに使い回す。
    fn parse_frame(
        &self,
        channel_samples: &mut Vec<Vec<i64>>,
    ) -> Result<(DecodedFrame, usize), ParseError> {
        let info = self
            .stream_info
            .as_ref()
            .expect("フレームフェーズでは STREAMINFO が存在する (実装バグ)");
        let mut reader = BitReader::new(self.pending());
        let header = FrameHeader::decode(&mut reader)?;

        // ブロッキング戦略はストリームを通して変わってはならない
        // (RFC 9639 Section 9.1: "MUST NOT change during the audio stream.")
        if let Some(strategy) = self.blocking_strategy
            && header.blocking_strategy != strategy
        {
            return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                "blocking strategy changed mid-stream (RFC 9639 Section 9.1)",
            ))));
        }

        // 符号化番号は先行するフレーム数 / サンプル数と一致しなければならない
        // (RFC 9639 Section 9.1.5)
        let expected_number = match header.blocking_strategy {
            BlockingStrategy::Fixed => self.frames_decoded,
            BlockingStrategy::Variable => self.samples_decoded,
        };
        if header.coded_number != expected_number {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "coded number {} does not match expected {} (RFC 9639 Section 9.1.5)",
                header.coded_number, expected_number
            ))));
        }

        // フレームのプロパティは STREAMINFO と一致しなければならない。
        // 一致しないフレームを許すとバッファ超過や MD5 不一致の原因になる
        // (RFC 9639 Section 9)
        let channels = header.channel_assignment.channels();
        if channels != info.channels {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "frame has {} channels but streaminfo has {} (RFC 9639 Section 9)",
                channels, info.channels
            ))));
        }
        let bits_per_sample = match header.bits_per_sample {
            Some(bits) => {
                if bits != info.bits_per_sample {
                    return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                        "frame has {} bits per sample but streaminfo has {} (RFC 9639 Section 9)",
                        bits, info.bits_per_sample
                    ))));
                }
                bits
            }
            None => info.bits_per_sample,
        };
        let sample_rate = match header.sample_rate {
            Some(rate) => {
                if rate != info.sample_rate {
                    return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                        "frame has sample rate {} but streaminfo has {} (RFC 9639 Section 9)",
                        rate, info.sample_rate
                    ))));
                }
                rate
            }
            None => info.sample_rate,
        };
        // ブロックサイズ 1-15 は最終フレーム以外で使ってはならない
        // (RFC 9639 Section 9.1.6: "only valid for the last frame in a stream
        // and MUST NOT be used for any other frame.")
        // ストリーミングデコーダーは最終フレームを事前に知り得ないため、
        // 少なくとも STREAMINFO の制約 (max_block_size >= 16) との整合は取れている。
        if header.block_size > info.max_block_size {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "frame block size {} exceeds streaminfo maximum {} (RFC 9639 Section 8.2)",
                header.block_size, info.max_block_size
            ))));
        }

        // sample_rate が 0 でオーディオフレームが存在するのは RFC 違反
        // (RFC 9639 Section 9.1.7: "MUST NOT be 0 when the subframe contains audio.")
        // STREAMINFO の sample_rate が 0 の悪意あるファイルは検出する
        if sample_rate == 0 {
            return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                "sample rate must not be 0 when audio is present (RFC 9639 Section 9.1.7)",
            ))));
        }

        // サブフレームをデコードする (RFC 9639 Section 9.2)
        channel_samples.resize_with(usize::from(channels), Vec::new);
        for (channel, samples) in channel_samples.iter_mut().enumerate() {
            // サイドチャンネルはビット深度が 1 bit 増える (RFC 9639 Section 9.2.3)
            let side_bit = u32::from(header.channel_assignment.is_side_channel(channel));
            decode_subframe(
                &mut reader,
                header.block_size,
                u32::from(bits_per_sample) + side_bit,
                samples,
            )?;
        }

        // フレームフッター: バイト境界までの 0 詰めと CRC-16 (RFC 9639 Section 9.3)
        let padding = reader.align_to_byte()?;
        if padding != 0 {
            return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                "frame padding bits must be zero (RFC 9639 Section 9.3)",
            ))));
        }
        let crc_offset = reader.position_bits() / 8;
        let actual = reader.read_u32(16)? as u16;
        let expected = crc16(reader.data_range(0, crc_offset));
        if expected != actual {
            return Err(ParseError::Invalid(DecodeError::FrameCrcMismatch {
                expected,
                actual,
            }));
        }

        // ステレオデコリレーションを元に戻す (RFC 9639 Section 4.2)
        undo_stereo_decorrelation(channel_samples, header.channel_assignment);

        // 全サンプルがビット深度の範囲に収まることを検証する (RFC 9639
        // Section 5)。範囲チェックをインターリーブから分離すると両方の
        // ループが分岐なしの連続走査になり、自動ベクトル化される。
        // エラーは稀なので、逸脱の検出後に値の特定をやり直せばよい
        let low = -(1i64 << (bits_per_sample - 1));
        let high = (1i64 << (bits_per_sample - 1)) - 1;
        for channel in channel_samples.iter() {
            let mut all_in_range = true;
            for &value in channel.iter() {
                // low <= value <= high は減算 1 回 + 符号なし比較 1 回と同値
                all_in_range &= (value.wrapping_sub(low) as u64) <= (high - low) as u64;
            }
            if !all_in_range {
                let value = channel
                    .iter()
                    .find(|&&v| v < low || v > high)
                    .expect("範囲逸脱を検出済み (実装バグ)");
                return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                    "decoded sample {} exceeds the range of {} bits per sample (RFC 9639 Section 5)",
                    value, bits_per_sample
                ))));
            }
        }

        // インターリーブして i32 に変換する。出力サイズはデコード済み
        // チャンネルバッファに実在するデータ量そのものなので、事前確保しても
        // 入力データ量に比例する
        let total = usize::from(header.block_size) * usize::from(channels);
        let mut samples = alloc::vec![0i32; total];
        match channel_samples.as_slice() {
            // 2 チャンネル (最も一般的) はペア書き込みに特殊化する
            [left, right] => {
                for ((out, &l), &r) in samples
                    .chunks_exact_mut(2)
                    .zip(left.iter())
                    .zip(right.iter())
                {
                    out[0] = l as i32;
                    out[1] = r as i32;
                }
            }
            _ => {
                for (channel_index, channel) in channel_samples.iter().enumerate() {
                    for (out, &value) in samples[channel_index..]
                        .iter_mut()
                        .step_by(usize::from(channels))
                        .zip(channel.iter())
                    {
                        *out = value as i32;
                    }
                }
            }
        }

        let frame = DecodedFrame {
            first_sample_number: self.samples_decoded,
            header,
            samples,
            sample_rate,
            bits_per_sample,
            channels,
        };
        Ok((frame, reader.position_bits() / 8))
    }
}

impl Default for StreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// ステレオデコリレーションを元に戻す (RFC 9639 Section 4.2)
fn undo_stereo_decorrelation(channels: &mut [Vec<i64>], assignment: ChannelAssignment) {
    match assignment {
        ChannelAssignment::Independent(_) => {}
        ChannelAssignment::LeftSide => {
            // right チャンネルを left と side から復元
            let (left, side) = channels.split_at_mut(1);
            for (r, &l) in side[0].iter_mut().zip(left[0].iter()) {
                *r = l - *r;
            }
        }
        ChannelAssignment::SideRight => {
            // left チャンネルを side と right から復元
            let (side, right) = channels.split_at_mut(1);
            for (l, &r) in side[0].iter_mut().zip(right[0].iter()) {
                *l += r;
            }
        }
        ChannelAssignment::MidSide => {
            // mid を 1 bit 左シフトし、side が奇数なら 1 を加えてから
            // l = (mid + side) >> 1、r = (mid - side) >> 1
            let (mid, side) = channels.split_at_mut(1);
            for (m, s) in mid[0].iter_mut().zip(side[0].iter_mut()) {
                let mid2 = (*m << 1) | (*s & 1);
                *m = (mid2 + *s) >> 1;
                *s = (mid2 - *s) >> 1;
            }
        }
    }
}

/// FLAC ストリーム全体を一括でデコードした結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedStream {
    /// STREAMINFO
    pub stream_info: StreamInfo,
    /// メタデータブロック列 (STREAMINFO 含む)
    pub metadata: Vec<MetadataBlock>,
    /// チャンネルインターリーブ済みの全サンプル
    pub samples: Vec<i32>,
    /// サンプルレート (Hz)
    pub sample_rate: u32,
    /// ビット深度
    pub bits_per_sample: u8,
    /// チャンネル数
    pub channels: u8,
}

/// FLAC ストリーム全体を一括でデコードする
///
/// ストリーミングが不要な場合の簡便 API。
pub fn decode(data: &[u8]) -> Result<DecodedStream, DecodeError> {
    let mut decoder = StreamDecoder::new();
    decoder.feed(data);
    decoder.finish();

    let mut samples = Vec::new();
    let mut sample_rate = 0;
    let mut bits_per_sample = 0;
    let mut channels = 0;
    while let Some(frame) = decoder.decode_frame()? {
        samples.extend_from_slice(&frame.samples);
        sample_rate = frame.sample_rate;
        bits_per_sample = frame.bits_per_sample;
        channels = frame.channels;
    }
    let stream_info = decoder
        .stream_info
        .take()
        .expect("decode_frame が成功していれば STREAMINFO は存在する (実装バグ)");
    if samples.is_empty() {
        // フレームが 1 つもないストリーム。STREAMINFO の値を使う
        sample_rate = stream_info.sample_rate;
        bits_per_sample = stream_info.bits_per_sample;
        channels = stream_info.channels;
    }
    Ok(DecodedStream {
        stream_info,
        metadata: decoder.metadata,
        samples,
        sample_rate,
        bits_per_sample,
        channels,
    })
}
