//! メタデータブロック (RFC 9639 Section 8)
//!
//! fLaC マーカー直後に並ぶメタデータブロックのデコード / エンコードを提供する。
//! 各ブロックは 4 バイトのヘッダー (last フラグ 1 bit + タイプ 7 bit +
//! サイズ 24 bit) とペイロードからなる (RFC 9639 Section 8.1)。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::bit_reader::{BitReadError, BitReader};
use crate::bit_writer::BitWriter;
use crate::cuesheet::Cuesheet;
use crate::error::{DecodeError, EncodeError};
use crate::picture::Picture;
use crate::vorbis_comment::VorbisComment;

/// メタデータブロックのペイロード最大サイズ (ヘッダーのサイズフィールドが 24 bit のため)
pub const MAX_METADATA_PAYLOAD_SIZE: usize = 0xFF_FFFF;

/// STREAMINFO メタデータブロック (RFC 9639 Section 8.2)
///
/// ストリーム全体の情報を保持する。FLAC ストリームの最初のメタデータブロックとして
/// 必ず存在しなければならない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    /// ストリーム中の最小ブロックサイズ (サンプル数、最終ブロックを除く)。16-65535
    pub min_block_size: u16,
    /// ストリーム中の最大ブロックサイズ (サンプル数)。16-65535
    pub max_block_size: u16,
    /// 最小フレームサイズ (バイト)。0 は不明を表す
    pub min_frame_size: u32,
    /// 最大フレームサイズ (バイト)。0 は不明を表す
    pub max_frame_size: u32,
    /// サンプルレート (Hz)。20 bit で表現できる 1-1048575。音声を含む場合 0 は不正
    pub sample_rate: u32,
    /// チャンネル数 (1-8)
    pub channels: u8,
    /// サンプルあたりのビット数 (4-32)
    pub bits_per_sample: u8,
    /// 総インターチャンネルサンプル数 (36 bit)。0 は不明を表す
    pub total_samples: u64,
    /// エンコード前オーディオデータの MD5 チェックサム。全て 0 は不明を表す
    pub md5: [u8; 16],
}

impl StreamInfo {
    /// STREAMINFO ペイロード (34 バイト) をデコードする
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        if payload.len() != 34 {
            return Err(DecodeError::InvalidData(format!(
                "streaminfo payload must be 34 bytes, got {} (RFC 9639 Section 8.2)",
                payload.len()
            )));
        }
        let mut reader = BitReader::new(payload);
        // 34 バイト確保済みなので以降の読み取りは失敗しない
        let eof = |_: BitReadError| {
            DecodeError::InvalidData(String::from("streaminfo payload underrun (unreachable)"))
        };
        let min_block_size = reader.read_u32(16).map_err(eof)? as u16;
        let max_block_size = reader.read_u32(16).map_err(eof)? as u16;
        let min_frame_size = reader.read_u32(24).map_err(eof)?;
        let max_frame_size = reader.read_u32(24).map_err(eof)?;
        let sample_rate = reader.read_u32(20).map_err(eof)?;
        let channels = reader.read_u32(3).map_err(eof)? as u8 + 1;
        let bits_per_sample = reader.read_u32(5).map_err(eof)? as u8 + 1;
        let total_samples = reader.read_u64(36).map_err(eof)?;
        let mut md5 = [0u8; 16];
        md5.copy_from_slice(reader.read_bytes(16).map_err(eof)?);

        // ブロックサイズ 16 未満は forbidden pattern (RFC 9639 Section 5 Table 1)
        if min_block_size < 16 || max_block_size < 16 {
            return Err(DecodeError::InvalidData(format!(
                "block size must be at least 16, got min={} max={} (RFC 9639 Section 8.2)",
                min_block_size, max_block_size
            )));
        }
        if min_block_size > max_block_size {
            return Err(DecodeError::InvalidData(format!(
                "min block size {} exceeds max block size {} (RFC 9639 Section 8.2)",
                min_block_size, max_block_size
            )));
        }
        // FLAC がサポートするビット深度は 4-32 (RFC 9639 Section 8.2 Table 3)
        if bits_per_sample < 4 {
            return Err(DecodeError::InvalidData(format!(
                "bits per sample must be 4-32, got {} (RFC 9639 Section 8.2)",
                bits_per_sample
            )));
        }

        Ok(Self {
            min_block_size,
            max_block_size,
            min_frame_size,
            max_frame_size,
            sample_rate,
            channels,
            bits_per_sample,
            total_samples,
            md5,
        })
    }

    /// STREAMINFO ペイロード (34 バイト) にエンコードする
    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        if self.min_block_size < 16 || self.max_block_size < 16 {
            return Err(EncodeError::InvalidMetadata(format!(
                "block size must be at least 16, got min={} max={} (RFC 9639 Section 8.2)",
                self.min_block_size, self.max_block_size
            )));
        }
        if self.min_block_size > self.max_block_size {
            return Err(EncodeError::InvalidMetadata(format!(
                "min block size {} exceeds max block size {} (RFC 9639 Section 8.2)",
                self.min_block_size, self.max_block_size
            )));
        }
        if self.min_frame_size > 0xFF_FFFF || self.max_frame_size > 0xFF_FFFF {
            return Err(EncodeError::InvalidMetadata(format!(
                "frame size must fit in 24 bits, got min={} max={} (RFC 9639 Section 8.2)",
                self.min_frame_size, self.max_frame_size
            )));
        }
        if self.sample_rate > 0xF_FFFF {
            return Err(EncodeError::InvalidMetadata(format!(
                "sample rate must fit in 20 bits, got {} (RFC 9639 Section 8.2)",
                self.sample_rate
            )));
        }
        if !(1..=8).contains(&self.channels) {
            return Err(EncodeError::InvalidMetadata(format!(
                "channels must be 1-8, got {} (RFC 9639 Section 8.2)",
                self.channels
            )));
        }
        if !(4..=32).contains(&self.bits_per_sample) {
            return Err(EncodeError::InvalidMetadata(format!(
                "bits per sample must be 4-32, got {} (RFC 9639 Section 8.2)",
                self.bits_per_sample
            )));
        }
        if self.total_samples > 0xF_FFFF_FFFF {
            return Err(EncodeError::InvalidMetadata(format!(
                "total samples must fit in 36 bits, got {} (RFC 9639 Section 8.2)",
                self.total_samples
            )));
        }

        let mut writer = BitWriter::new();
        writer.write_u32(u32::from(self.min_block_size), 16);
        writer.write_u32(u32::from(self.max_block_size), 16);
        writer.write_u32(self.min_frame_size, 24);
        writer.write_u32(self.max_frame_size, 24);
        writer.write_u32(self.sample_rate, 20);
        writer.write_u32(u32::from(self.channels - 1), 3);
        writer.write_u32(u32::from(self.bits_per_sample - 1), 5);
        writer.write_u64(self.total_samples, 36);
        writer.write_bytes(&self.md5);
        Ok(writer.into_bytes())
    }
}

/// APPLICATION メタデータブロック (RFC 9639 Section 8.4)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    /// IANA 登録のアプリケーション ID
    pub id: [u8; 4],
    /// アプリケーション固有データ
    pub data: Vec<u8>,
}

impl Application {
    /// APPLICATION ペイロードをデコードする
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        if payload.len() < 4 {
            return Err(DecodeError::InvalidData(format!(
                "application block must have a 4-byte application ID, got {} bytes (RFC 9639 Section 8.4)",
                payload.len()
            )));
        }
        let mut id = [0u8; 4];
        id.copy_from_slice(&payload[..4]);
        Ok(Self {
            id,
            data: payload[4..].to_vec(),
        })
    }

    /// APPLICATION ペイロードにエンコードする
    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.id);
        out.extend_from_slice(&self.data);
        if out.len() > MAX_METADATA_PAYLOAD_SIZE {
            return Err(EncodeError::InvalidMetadata(format!(
                "application block payload {} bytes exceeds 24-bit size limit (RFC 9639 Section 8.1)",
                out.len()
            )));
        }
        Ok(out)
    }
}

/// シークポイント (RFC 9639 Section 8.5.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekPoint {
    /// 対象フレームの最初のサンプル番号。プレースホルダーは 0xFFFFFFFFFFFFFFFF
    pub sample_number: u64,
    /// 最初のフレームヘッダーから対象フレームヘッダーへのバイトオフセット
    pub stream_offset: u64,
    /// 対象フレームのサンプル数
    pub frame_samples: u16,
}

impl SeekPoint {
    /// プレースホルダーポイントのサンプル番号 (RFC 9639 Section 8.5.1)
    pub const PLACEHOLDER: u64 = u64::MAX;

    /// プレースホルダーポイントか
    pub fn is_placeholder(&self) -> bool {
        self.sample_number == Self::PLACEHOLDER
    }
}

/// SEEKTABLE メタデータブロック (RFC 9639 Section 8.5)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeekTable {
    /// シークポイント列 (サンプル番号昇順、プレースホルダーは末尾)
    pub points: Vec<SeekPoint>,
}

/// シークポイント列の構造検証 (RFC 9639 Section 8.5.1)
///
/// サンプル番号は昇順・一意 (プレースホルダー除く) で、プレースホルダーは
/// 末尾にまとめて置かれていなければならない。違反時は理由を返す。
fn validate_seek_points(points: &[SeekPoint]) -> Result<(), String> {
    let mut prev: Option<u64> = None;
    let mut seen_placeholder = false;
    for point in points {
        if point.is_placeholder() {
            seen_placeholder = true;
            continue;
        }
        if seen_placeholder {
            return Err(String::from(
                "placeholder seek points must all occur at the end (RFC 9639 Section 8.5.1)",
            ));
        }
        if let Some(prev) = prev
            && point.sample_number <= prev
        {
            return Err(format!(
                "seek points must be sorted and unique by sample number, got {} after {} (RFC 9639 Section 8.5.1)",
                point.sample_number, prev
            ));
        }
        prev = Some(point.sample_number);
    }
    Ok(())
}

impl SeekTable {
    /// SEEKTABLE ペイロードをデコードする
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        // 各シークポイントは 18 バイト。個数はペイロードサイズから導出する
        // (RFC 9639 Section 8.5)
        if !payload.len().is_multiple_of(18) {
            return Err(DecodeError::InvalidData(format!(
                "seek table payload size {} is not a multiple of 18 (RFC 9639 Section 8.5)",
                payload.len()
            )));
        }
        let mut points = Vec::new();
        // ペイロード長は 18 の倍数であることを確認済みなので端数は出ない
        let (chunks, _) = payload.as_chunks::<18>();
        for chunk in chunks {
            let sample_number =
                u64::from_be_bytes(chunk[0..8].try_into().expect("8 バイト固定 (実装バグ)"));
            let stream_offset =
                u64::from_be_bytes(chunk[8..16].try_into().expect("8 バイト固定 (実装バグ)"));
            let frame_samples =
                u16::from_be_bytes(chunk[16..18].try_into().expect("2 バイト固定 (実装バグ)"));
            points.push(SeekPoint {
                sample_number,
                stream_offset,
                frame_samples,
            });
        }
        validate_seek_points(&points).map_err(DecodeError::InvalidData)?;
        Ok(Self { points })
    }

    /// SEEKTABLE ペイロードにエンコードする
    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        if self.points.len() * 18 > MAX_METADATA_PAYLOAD_SIZE {
            return Err(EncodeError::InvalidMetadata(format!(
                "seek table with {} points exceeds 24-bit size limit (RFC 9639 Section 8.1)",
                self.points.len()
            )));
        }
        validate_seek_points(&self.points).map_err(EncodeError::InvalidMetadata)?;

        let mut out = Vec::new();
        for point in &self.points {
            out.extend_from_slice(&point.sample_number.to_be_bytes());
            out.extend_from_slice(&point.stream_offset.to_be_bytes());
            out.extend_from_slice(&point.frame_samples.to_be_bytes());
        }
        Ok(out)
    }
}

/// メタデータブロック (RFC 9639 Section 8.1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataBlock {
    /// STREAMINFO (タイプ 0)
    StreamInfo(StreamInfo),
    /// PADDING (タイプ 1)。size はペイロードのバイト数
    Padding { size: u32 },
    /// APPLICATION (タイプ 2)
    Application(Application),
    /// SEEKTABLE (タイプ 3)
    SeekTable(SeekTable),
    /// VORBIS_COMMENT (タイプ 4)
    VorbisComment(VorbisComment),
    /// CUESHEET (タイプ 5)
    Cuesheet(Cuesheet),
    /// PICTURE (タイプ 6)
    Picture(Picture),
    /// 予約済みタイプ (7-126)。将来の拡張のため中身をそのまま保持する
    Unknown { block_type: u8, data: Vec<u8> },
}

impl MetadataBlock {
    /// ブロックタイプとペイロードからメタデータブロックをデコードする
    pub fn decode(block_type: u8, payload: &[u8]) -> Result<Self, DecodeError> {
        match block_type {
            0 => Ok(MetadataBlock::StreamInfo(StreamInfo::decode(payload)?)),
            1 => {
                // PADDING のペイロードは全て 0 ビット (RFC 9639 Section 8.3)。
                // 中身が 0 でなくても致命的ではないため検証はしない
                Ok(MetadataBlock::Padding {
                    size: payload.len() as u32,
                })
            }
            2 => Ok(MetadataBlock::Application(Application::decode(payload)?)),
            3 => Ok(MetadataBlock::SeekTable(SeekTable::decode(payload)?)),
            4 => Ok(MetadataBlock::VorbisComment(VorbisComment::decode(
                payload,
            )?)),
            5 => Ok(MetadataBlock::Cuesheet(Cuesheet::decode(payload)?)),
            6 => Ok(MetadataBlock::Picture(Picture::decode(payload)?)),
            7..=126 => Ok(MetadataBlock::Unknown {
                block_type,
                data: payload.to_vec(),
            }),
            _ => Err(DecodeError::InvalidData(format!(
                "metadata block type {} is forbidden (RFC 9639 Section 8.1)",
                block_type
            ))),
        }
    }

    /// ブロックタイプ番号 (RFC 9639 Section 8.1 Table 2)
    pub fn block_type(&self) -> u8 {
        match self {
            MetadataBlock::StreamInfo(_) => 0,
            MetadataBlock::Padding { .. } => 1,
            MetadataBlock::Application(_) => 2,
            MetadataBlock::SeekTable(_) => 3,
            MetadataBlock::VorbisComment(_) => 4,
            MetadataBlock::Cuesheet(_) => 5,
            MetadataBlock::Picture(_) => 6,
            MetadataBlock::Unknown { block_type, .. } => *block_type,
        }
    }

    /// ペイロードにエンコードする (ブロックヘッダーは含まない)
    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        let payload = match self {
            MetadataBlock::StreamInfo(info) => info.encode_payload()?,
            MetadataBlock::Padding { size } => {
                if *size as usize > MAX_METADATA_PAYLOAD_SIZE {
                    return Err(EncodeError::InvalidMetadata(format!(
                        "padding size {} exceeds 24-bit size limit (RFC 9639 Section 8.1)",
                        size
                    )));
                }
                alloc::vec![0u8; *size as usize]
            }
            MetadataBlock::Application(app) => app.encode_payload()?,
            MetadataBlock::SeekTable(table) => table.encode_payload()?,
            MetadataBlock::VorbisComment(comment) => comment.encode_payload()?,
            MetadataBlock::Cuesheet(cuesheet) => cuesheet.encode_payload()?,
            MetadataBlock::Picture(picture) => picture.encode_payload()?,
            MetadataBlock::Unknown { block_type, data } => {
                if *block_type >= 127 {
                    return Err(EncodeError::InvalidMetadata(format!(
                        "metadata block type {} is forbidden (RFC 9639 Section 8.1)",
                        block_type
                    )));
                }
                data.clone()
            }
        };
        if payload.len() > MAX_METADATA_PAYLOAD_SIZE {
            return Err(EncodeError::InvalidMetadata(format!(
                "metadata payload {} bytes exceeds 24-bit size limit (RFC 9639 Section 8.1)",
                payload.len()
            )));
        }
        Ok(payload)
    }

    /// ブロックヘッダー込みでエンコードする (RFC 9639 Section 8.1)
    pub fn encode(&self, is_last: bool) -> Result<Vec<u8>, EncodeError> {
        let payload = self.encode_payload()?;
        let mut out = Vec::new();
        let last_bit = if is_last { 0x80 } else { 0x00 };
        out.push(last_bit | self.block_type());
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..4]);
        out.extend_from_slice(&payload);
        Ok(out)
    }
}
