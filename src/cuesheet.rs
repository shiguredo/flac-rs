//! CUESHEET メタデータブロック (RFC 9639 Section 8.7)
//!
//! CD-DA のトラック / インデックスポイント構造、または FLAC ファイル内の
//! 任意の位置情報を保持する。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::bit_reader::{BitReadError, BitReader};
use crate::bit_writer::BitWriter;
use crate::error::{DecodeError, EncodeError};
use crate::metadata::MAX_METADATA_PAYLOAD_SIZE;

/// CUESHEET トラックのインデックスポイント (RFC 9639 Section 8.7.1.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CuesheetTrackIndex {
    /// トラックオフセットからの相対オフセット (サンプル数)
    pub offset_samples: u64,
    /// インデックスポイント番号
    pub number: u8,
}

/// CUESHEET のトラック (RFC 9639 Section 8.7.1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuesheetTrack {
    /// FLAC オーディオストリーム先頭からの最初のインデックスポイントのオフセット (サンプル数)
    pub offset_samples: u64,
    /// トラック番号 (0 は不正)
    pub number: u8,
    /// トラックの ISRC。無い場合は全て 0x00
    pub isrc: [u8; 12],
    /// トラックタイプ: false = オーディオ、true = 非オーディオ
    pub is_non_audio: bool,
    /// プリエンファシスフラグ
    pub pre_emphasis: bool,
    /// インデックスポイント列 (リードアウトトラックは空でなければならない)
    pub index_points: Vec<CuesheetTrackIndex>,
}

/// CUESHEET メタデータブロック (RFC 9639 Section 8.7)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cuesheet {
    /// メディアカタログ番号 (ASCII 印字可能文字 0x20-0x7E、右側 0x00 パディング)
    pub media_catalog_number: [u8; 128],
    /// リードインのサンプル数 (CD-DA 以外では 0)
    pub lead_in_samples: u64,
    /// CD-DA に対応するキューシートか
    pub is_cdda: bool,
    /// トラック列 (最後はリードアウトトラック)
    pub tracks: Vec<CuesheetTrack>,
}

/// メディアカタログ番号の検証 (RFC 9639 Section 8.7)
///
/// 印字可能 ASCII 0x20-0x7E で、右側は 0x00 でパディングされていること。
fn validate_media_catalog_number(mcn: &[u8; 128]) -> Result<(), String> {
    let end = mcn.iter().position(|&b| b == 0).unwrap_or(128);
    if !mcn[..end].iter().all(|&b| (0x20..=0x7E).contains(&b))
        || !mcn[end..].iter().all(|&b| b == 0)
    {
        return Err(String::from(
            "media catalog number must be printable ASCII padded with NUL (RFC 9639 Section 8.7)",
        ));
    }
    Ok(())
}

/// トラック列の構造検証 (RFC 9639 Section 8.7, 8.7.1, 8.7.1.1)
///
/// - リードアウトトラックが必須のため最低 1 トラック
/// - トラック番号 0 は禁止で、番号はキューシート内で一意
/// - リードアウト (最終) トラックのインデックスポイントは 0 個、
///   それ以外のトラックには 1 個以上
/// - 各トラックのインデックスポイント番号は 0 または 1 から始まる連番
fn validate_tracks(tracks: &[CuesheetTrack]) -> Result<(), String> {
    if tracks.is_empty() {
        return Err(String::from(
            "cuesheet must have at least a lead-out track (RFC 9639 Section 8.7)",
        ));
    }
    let mut seen_numbers = alloc::collections::BTreeSet::new();
    for (i, track) in tracks.iter().enumerate() {
        if track.number == 0 {
            return Err(String::from(
                "cuesheet track number 0 is not allowed (RFC 9639 Section 8.7.1)",
            ));
        }
        if !seen_numbers.insert(track.number) {
            return Err(format!(
                "cuesheet track number {} is not unique (RFC 9639 Section 8.7.1)",
                track.number
            ));
        }
        let is_lead_out = i + 1 == tracks.len();
        if is_lead_out {
            if !track.index_points.is_empty() {
                return Err(String::from(
                    "lead-out track must have zero index points (RFC 9639 Section 8.7.1)",
                ));
            }
            continue;
        }
        if track.index_points.is_empty() {
            return Err(String::from(
                "every track except the lead-out must have at least one index point (RFC 9639 Section 8.7.1)",
            ));
        }
        // 最初のインデックスポイント番号は 0 または 1、以降は 1 ずつ増える
        // (RFC 9639 Section 8.7.1.1)
        let first = track.index_points[0].number;
        if first > 1 {
            return Err(format!(
                "first index point number must be 0 or 1, got {} (RFC 9639 Section 8.7.1.1)",
                first
            ));
        }
        for (j, index) in track.index_points.iter().enumerate().skip(1) {
            let expected = usize::from(first) + j;
            if usize::from(index.number) != expected {
                return Err(format!(
                    "index point numbers must increase by 1, got {} where {} was expected (RFC 9639 Section 8.7.1.1)",
                    index.number, expected
                ));
            }
        }
    }
    Ok(())
}

impl Cuesheet {
    /// メディアカタログ番号を文字列として返す (末尾の 0x00 パディングを除く)
    pub fn media_catalog_number_str(&self) -> &str {
        let end = self
            .media_catalog_number
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(128);
        // ASCII 印字可能文字のみをデコード時に検証しているため常に有効な UTF-8
        core::str::from_utf8(&self.media_catalog_number[..end])
            .expect("カタログ番号は検証済みの ASCII (実装バグ)")
    }

    /// CUESHEET ペイロードをデコードする
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = BitReader::new(payload);
        let eof = |_: BitReadError| {
            DecodeError::InvalidData(String::from("cuesheet is truncated (RFC 9639 Section 8.7)"))
        };

        let mut media_catalog_number = [0u8; 128];
        media_catalog_number.copy_from_slice(reader.read_bytes(128).map_err(eof)?);
        validate_media_catalog_number(&media_catalog_number).map_err(DecodeError::InvalidData)?;

        let lead_in_samples = reader.read_u64(64).map_err(eof)?;
        let is_cdda = reader.read_bit().map_err(eof)?;
        // 予約領域 7 + 258*8 bit は全て 0 でなければならない (RFC 9639 Section 8.7)
        let reserved = reader.read_u64(7).map_err(eof)?;
        let reserved_bytes = reader.read_bytes(258).map_err(eof)?;
        if reserved != 0 || reserved_bytes.iter().any(|&b| b != 0) {
            return Err(DecodeError::InvalidData(String::from(
                "cuesheet reserved bits must be zero (RFC 9639 Section 8.7)",
            )));
        }

        let track_count = reader.read_u32(8).map_err(eof)? as usize;
        let mut tracks = Vec::new();
        for _ in 0..track_count {
            let offset_samples = reader.read_u64(64).map_err(eof)?;
            let number = reader.read_u32(8).map_err(eof)? as u8;
            let mut isrc = [0u8; 12];
            isrc.copy_from_slice(reader.read_bytes(12).map_err(eof)?);
            let is_non_audio = reader.read_bit().map_err(eof)?;
            let pre_emphasis = reader.read_bit().map_err(eof)?;
            // 予約領域 6 + 13*8 bit は全て 0 でなければならない
            // (RFC 9639 Section 8.7.1)
            let reserved = reader.read_u64(6).map_err(eof)?;
            let reserved_bytes = reader.read_bytes(13).map_err(eof)?;
            if reserved != 0 || reserved_bytes.iter().any(|&b| b != 0) {
                return Err(DecodeError::InvalidData(String::from(
                    "cuesheet track reserved bits must be zero (RFC 9639 Section 8.7.1)",
                )));
            }

            let index_count = reader.read_u32(8).map_err(eof)? as usize;
            let mut index_points = Vec::new();
            for _ in 0..index_count {
                let index_offset = reader.read_u64(64).map_err(eof)?;
                let index_number = reader.read_u32(8).map_err(eof)? as u8;
                // 予約領域 3*8 bit は全て 0 でなければならない
                // (RFC 9639 Section 8.7.1.1)
                let reserved_bytes = reader.read_bytes(3).map_err(eof)?;
                if reserved_bytes.iter().any(|&b| b != 0) {
                    return Err(DecodeError::InvalidData(String::from(
                        "cuesheet track index reserved bits must be zero (RFC 9639 Section 8.7.1.1)",
                    )));
                }
                index_points.push(CuesheetTrackIndex {
                    offset_samples: index_offset,
                    number: index_number,
                });
            }

            tracks.push(CuesheetTrack {
                offset_samples,
                number,
                isrc,
                is_non_audio,
                pre_emphasis,
                index_points,
            });
        }

        if !reader.is_byte_aligned() || reader.position_bits() / 8 != payload.len() {
            return Err(DecodeError::InvalidData(format!(
                "cuesheet has {} trailing bytes (RFC 9639 Section 8.7)",
                payload.len() - reader.position_bits() / 8
            )));
        }
        validate_tracks(&tracks).map_err(DecodeError::InvalidData)?;

        Ok(Self {
            media_catalog_number,
            lead_in_samples,
            is_cdda,
            tracks,
        })
    }

    /// CUESHEET ペイロードにエンコードする
    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        validate_media_catalog_number(&self.media_catalog_number)
            .map_err(EncodeError::InvalidMetadata)?;
        validate_tracks(&self.tracks).map_err(EncodeError::InvalidMetadata)?;
        if self.tracks.len() > 255 {
            return Err(EncodeError::InvalidMetadata(format!(
                "cuesheet cannot have more than 255 tracks, got {} (RFC 9639 Section 8.7)",
                self.tracks.len()
            )));
        }

        let mut writer = BitWriter::new();
        writer.write_bytes(&self.media_catalog_number);
        writer.write_u64(self.lead_in_samples, 64);
        writer.write_u32(u32::from(self.is_cdda), 1);
        // 予約領域 7 + 258*8 bit は全て 0 (RFC 9639 Section 8.7)
        writer.write_u32(0, 7);
        writer.write_bytes(&[0u8; 258]);
        writer.write_u32(self.tracks.len() as u32, 8);

        for track in &self.tracks {
            if track.index_points.len() > 255 {
                return Err(EncodeError::InvalidMetadata(format!(
                    "cuesheet track cannot have more than 255 index points, got {} (RFC 9639 Section 8.7.1)",
                    track.index_points.len()
                )));
            }
            writer.write_u64(track.offset_samples, 64);
            writer.write_u32(u32::from(track.number), 8);
            writer.write_bytes(&track.isrc);
            writer.write_u32(u32::from(track.is_non_audio), 1);
            writer.write_u32(u32::from(track.pre_emphasis), 1);
            // 予約領域 6 + 13*8 bit
            writer.write_u32(0, 6);
            writer.write_bytes(&[0u8; 13]);
            writer.write_u32(track.index_points.len() as u32, 8);
            for index in &track.index_points {
                writer.write_u64(index.offset_samples, 64);
                writer.write_u32(u32::from(index.number), 8);
                // 予約領域 3*8 bit
                writer.write_bytes(&[0u8; 3]);
            }
        }

        let out = writer.into_bytes();
        if out.len() > MAX_METADATA_PAYLOAD_SIZE {
            return Err(EncodeError::InvalidMetadata(format!(
                "cuesheet payload {} bytes exceeds 24-bit size limit (RFC 9639 Section 8.1)",
                out.len()
            )));
        }
        Ok(out)
    }
}
