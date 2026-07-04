//! フレームヘッダー / フッター (RFC 9639 Section 9.1, 9.3)
//!
//! 各フレームはバイト境界から 15 bit の同期コード 0b111111111111100 で始まり、
//! ブロッキング戦略・ブロックサイズ・サンプルレート・チャンネル割り当て・
//! ビット深度・符号化番号 (フレーム番号またはサンプル番号) と CRC-8 が続く。

use alloc::format;
use alloc::string::String;

use crate::bit_reader::BitReader;
use crate::bit_writer::BitWriter;
use crate::crc::crc8;
use crate::error::{DecodeError, EncodeError, ParseError};

/// フレーム同期コード (15 bit) (RFC 9639 Section 9.1)
pub(crate) const FRAME_SYNC_CODE: u32 = 0b111_1111_1111_1100;

/// ブロッキング戦略 (RFC 9639 Section 9.1)
///
/// ストリーム全体を通して変わってはならない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockingStrategy {
    /// 固定ブロックサイズ。符号化番号はフレーム番号
    Fixed,
    /// 可変ブロックサイズ。符号化番号はサンプル番号
    Variable,
}

/// チャンネル割り当て (RFC 9639 Section 9.1.3 Table 16)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelAssignment {
    /// 全チャンネル独立 (1-8 チャンネル)
    Independent(u8),
    /// 2 チャンネル: 左 + サイド (left-side stereo)
    LeftSide,
    /// 2 チャンネル: サイド + 右 (side-right stereo)
    SideRight,
    /// 2 チャンネル: ミッド + サイド (mid-side stereo)
    MidSide,
}

impl ChannelAssignment {
    /// チャンネル数
    pub fn channels(&self) -> u8 {
        match self {
            ChannelAssignment::Independent(n) => *n,
            ChannelAssignment::LeftSide
            | ChannelAssignment::SideRight
            | ChannelAssignment::MidSide => 2,
        }
    }

    /// 指定チャンネルがサイドチャンネル (ビット深度 +1) か (RFC 9639 Section 9.2.3)
    pub(crate) fn is_side_channel(&self, channel: usize) -> bool {
        match self {
            ChannelAssignment::Independent(_) => false,
            // left-side: サブフレーム 1 がサイド
            ChannelAssignment::LeftSide => channel == 1,
            // side-right: サブフレーム 0 がサイド
            ChannelAssignment::SideRight => channel == 0,
            // mid-side: サブフレーム 1 がサイド
            ChannelAssignment::MidSide => channel == 1,
        }
    }

    /// チャンネルビット (RFC 9639 Section 9.1.3 Table 16)
    fn to_bits(self) -> u32 {
        match self {
            ChannelAssignment::Independent(n) => u32::from(n) - 1,
            ChannelAssignment::LeftSide => 0b1000,
            ChannelAssignment::SideRight => 0b1001,
            ChannelAssignment::MidSide => 0b1010,
        }
    }
}

/// フレームヘッダー (RFC 9639 Section 9.1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    /// ブロッキング戦略
    pub blocking_strategy: BlockingStrategy,
    /// ブロックサイズ (インターチャンネルサンプル数)
    pub block_size: u16,
    /// サンプルレート (Hz)。`None` は STREAMINFO 参照
    pub sample_rate: Option<u32>,
    /// チャンネル割り当て
    pub channel_assignment: ChannelAssignment,
    /// ビット深度。`None` は STREAMINFO 参照
    pub bits_per_sample: Option<u8>,
    /// 符号化番号: 固定ブロックサイズならフレーム番号、可変ならサンプル番号
    /// (RFC 9639 Section 9.1.5)
    pub coded_number: u64,
}

/// 符号化番号 (UTF-8 拡張形式、最大 36 bit / 7 バイト) をデコードする
/// (RFC 9639 Section 9.1.5 Table 18)
fn decode_coded_number(reader: &mut BitReader<'_>) -> Result<u64, ParseError> {
    let first = reader.read_u32(8)?;
    // 先頭バイトの上位ビットから後続バイト数を求める
    let (extra_bytes, mut value) = match first {
        0b0000_0000..=0b0111_1111 => (0u32, u64::from(first)),
        0b1100_0000..=0b1101_1111 => (1, u64::from(first & 0b0001_1111)),
        0b1110_0000..=0b1110_1111 => (2, u64::from(first & 0b0000_1111)),
        0b1111_0000..=0b1111_0111 => (3, u64::from(first & 0b0000_0111)),
        0b1111_1000..=0b1111_1011 => (4, u64::from(first & 0b0000_0011)),
        0b1111_1100..=0b1111_1101 => (5, u64::from(first & 0b0000_0001)),
        0b1111_1110 => (6, 0),
        _ => {
            // 0b10xxxxxx (継続バイト) と 0b11111111 は先頭バイトとして不正
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "invalid coded number leading byte {:#04x} (RFC 9639 Section 9.1.5)",
                first
            ))));
        }
    };
    for _ in 0..extra_bytes {
        let byte = reader.read_u32(8)?;
        if byte & 0b1100_0000 != 0b1000_0000 {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "invalid coded number continuation byte {:#04x} (RFC 9639 Section 9.1.5)",
                byte
            ))));
        }
        value = (value << 6) | u64::from(byte & 0b0011_1111);
    }
    // RFC 3629 Section 3 の手続きに従い、最短形式でない符号化は不正とする
    let min_value = match extra_bytes {
        0 => 0,
        1 => 0x80,
        2 => 0x800,
        3 => 0x1_0000,
        4 => 0x20_0000,
        5 => 0x400_0000,
        _ => 0x8000_0000,
    };
    if value < min_value {
        return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
            "coded number {} uses a non-shortest encoding (RFC 9639 Section 9.1.5)",
            value
        ))));
    }
    Ok(value)
}

/// 符号化番号をエンコードする (RFC 9639 Section 9.1.5 Table 18)
///
/// RFC 3629 の手続きに従い、値が収まる最短のオクテット列を使う。
/// 呼び出し側で値が 36 bit に収まることを保証すること。
pub(crate) fn encode_coded_number(writer: &mut BitWriter, value: u64) {
    debug_assert!(
        value <= 0xF_FFFF_FFFF,
        "符号化番号は 36 bit まで (実装バグ)"
    );
    // (先頭バイトのプレフィックス, 全バイト数, 値のビット数) の組
    let (prefix, total_bytes) = match value {
        0..=0x7F => (0b0000_0000u32, 1u32),
        0x80..=0x7FF => (0b1100_0000, 2),
        0x800..=0xFFFF => (0b1110_0000, 3),
        0x1_0000..=0x1F_FFFF => (0b1111_0000, 4),
        0x20_0000..=0x3FF_FFFF => (0b1111_1000, 5),
        0x400_0000..=0x7FFF_FFFF => (0b1111_1100, 6),
        _ => (0b1111_1110, 7),
    };
    let continuation_bytes = total_bytes - 1;
    // 先頭バイト: プレフィックス + 値の最上位ビット群
    let leading_value_bits = match total_bytes {
        1 => 7,
        7 => 0,
        n => 7 - n,
    };
    let leading =
        prefix | ((value >> (6 * continuation_bytes)) as u32 & ((1 << leading_value_bits) - 1));
    writer.write_u32(leading, 8);
    // 継続バイト: 0b10 + 6 bit ずつ
    for i in (0..continuation_bytes).rev() {
        let bits = ((value >> (6 * i)) & 0b0011_1111) as u32;
        writer.write_u32(0b1000_0000 | bits, 8);
    }
}

impl FrameHeader {
    /// フレームヘッダーをデコードする
    ///
    /// `reader` はバイト境界のフレーム先頭を指していること。CRC-8 の検証まで行う。
    pub(crate) fn decode(reader: &mut BitReader<'_>) -> Result<Self, ParseError> {
        debug_assert!(reader.is_byte_aligned(), "フレームはバイト境界から始まる");
        let header_start = reader.position_bits() / 8;

        let sync = reader.read_u32(15)?;
        if sync != FRAME_SYNC_CODE {
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "invalid frame sync code {:#06x} (RFC 9639 Section 9.1)",
                sync
            ))));
        }
        let blocking_strategy = if reader.read_bit()? {
            BlockingStrategy::Variable
        } else {
            BlockingStrategy::Fixed
        };

        let block_size_bits = reader.read_u32(4)?;
        let sample_rate_bits = reader.read_u32(4)?;
        let channel_bits = reader.read_u32(4)?;
        let bit_depth_bits = reader.read_u32(3)?;
        let reserved = reader.read_bit()?;
        if reserved {
            return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                "frame header reserved bit must be zero (RFC 9639 Section 9.1.4)",
            ))));
        }

        let coded_number = decode_coded_number(reader)?;
        if blocking_strategy == BlockingStrategy::Fixed && coded_number > 0x7FFF_FFFF {
            // フレーム番号は 31 bit まで (RFC 9639 Section 9.1.5)
            return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                "frame number {} exceeds 31 bits (RFC 9639 Section 9.1.5)",
                coded_number
            ))));
        }

        // ブロックサイズ (RFC 9639 Section 9.1.1 Table 14)
        let block_size = match block_size_bits {
            0b0000 => {
                return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                    "block size bits 0b0000 is reserved (RFC 9639 Section 9.1.1)",
                ))));
            }
            0b0001 => 192,
            v @ 0b0010..=0b0101 => 144 * (1 << v),
            // uncommon block size は符号化番号の後に格納される (RFC 9639 Section 9.1.6)
            0b0110 => reader.read_u32(8)? as u16 + 1,
            0b0111 => {
                let stored = reader.read_u32(16)?;
                if stored == 65535 {
                    // ブロックサイズ 65536 は forbidden (RFC 9639 Section 9.1.6)
                    return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                        "uncommon block size 65536 is forbidden (RFC 9639 Section 9.1.6)",
                    ))));
                }
                stored as u16 + 1
            }
            v => 1 << v,
        };

        // サンプルレート (RFC 9639 Section 9.1.2 Table 15)
        let sample_rate = match sample_rate_bits {
            0b0000 => None,
            0b0001 => Some(88_200),
            0b0010 => Some(176_400),
            0b0011 => Some(192_000),
            0b0100 => Some(8_000),
            0b0101 => Some(16_000),
            0b0110 => Some(22_050),
            0b0111 => Some(24_000),
            0b1000 => Some(32_000),
            0b1001 => Some(44_100),
            0b1010 => Some(48_000),
            0b1011 => Some(96_000),
            // uncommon sample rate は uncommon block size の後に格納される
            // (RFC 9639 Section 9.1.7)
            0b1100 => Some(reader.read_u32(8)? * 1000),
            0b1101 => Some(reader.read_u32(16)?),
            0b1110 => Some(reader.read_u32(16)? * 10),
            _ => {
                return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                    "sample rate bits 0b1111 is forbidden (RFC 9639 Section 9.1.2)",
                ))));
            }
        };

        // チャンネル割り当て (RFC 9639 Section 9.1.3 Table 16)
        let channel_assignment = match channel_bits {
            0b0000..=0b0111 => ChannelAssignment::Independent(channel_bits as u8 + 1),
            0b1000 => ChannelAssignment::LeftSide,
            0b1001 => ChannelAssignment::SideRight,
            0b1010 => ChannelAssignment::MidSide,
            _ => {
                return Err(ParseError::Invalid(DecodeError::InvalidData(format!(
                    "channel bits {:#06b} is reserved (RFC 9639 Section 9.1.3)",
                    channel_bits
                ))));
            }
        };

        // ビット深度 (RFC 9639 Section 9.1.4 Table 17)
        let bits_per_sample = match bit_depth_bits {
            0b000 => None,
            0b001 => Some(8),
            0b010 => Some(12),
            0b011 => {
                return Err(ParseError::Invalid(DecodeError::InvalidData(String::from(
                    "bit depth bits 0b011 is reserved (RFC 9639 Section 9.1.4)",
                ))));
            }
            0b100 => Some(16),
            0b101 => Some(20),
            0b110 => Some(24),
            _ => Some(32),
        };

        // CRC-8 はヘッダー先頭 (同期コード含む) から CRC 自身の直前までを保護する
        // (RFC 9639 Section 9.1.8)
        debug_assert!(
            reader.is_byte_aligned(),
            "CRC-8 の位置はバイト境界 (実装バグ)"
        );
        let header_end = reader.position_bits() / 8;
        let actual = reader.read_u32(8)? as u8;
        // reader が保持するスライス全体からヘッダー部分を取り出して CRC を計算する
        let expected = crc8(reader.data_range(header_start, header_end));
        if expected != actual {
            return Err(ParseError::Invalid(DecodeError::FrameHeaderCrcMismatch {
                expected,
                actual,
            }));
        }

        Ok(Self {
            blocking_strategy,
            block_size,
            sample_rate,
            channel_assignment,
            bits_per_sample,
            coded_number,
        })
    }

    /// フレームヘッダーをエンコードする (CRC-8 込み)
    pub(crate) fn encode(&self, writer: &mut BitWriter) -> Result<(), EncodeError> {
        debug_assert!(writer.is_byte_aligned(), "フレームはバイト境界から始まる");
        let header_start = writer.byte_len();

        // ブロックサイズのビット表現と uncommon 値 (RFC 9639 Section 9.1.1)
        let (block_size_bits, uncommon_block_size) = match self.block_size {
            0 => {
                return Err(EncodeError::InvalidConfig(String::from(
                    "block size must not be zero",
                )));
            }
            192 => (0b0001, None),
            576 => (0b0010, None),
            1152 => (0b0011, None),
            2304 => (0b0100, None),
            4608 => (0b0101, None),
            256 => (0b1000, None),
            512 => (0b1001, None),
            1024 => (0b1010, None),
            2048 => (0b1011, None),
            4096 => (0b1100, None),
            8192 => (0b1101, None),
            16384 => (0b1110, None),
            32768 => (0b1111, None),
            n if n <= 256 => (0b0110, Some(u32::from(n - 1))),
            n => (0b0111, Some(u32::from(n - 1))),
        };

        // サンプルレートのビット表現と uncommon 値 (RFC 9639 Section 9.1.2)
        let (sample_rate_bits, uncommon_sample_rate) = match self.sample_rate {
            None => (0b0000, None),
            Some(88_200) => (0b0001, None),
            Some(176_400) => (0b0010, None),
            Some(192_000) => (0b0011, None),
            Some(8_000) => (0b0100, None),
            Some(16_000) => (0b0101, None),
            Some(22_050) => (0b0110, None),
            Some(24_000) => (0b0111, None),
            Some(32_000) => (0b1000, None),
            Some(44_100) => (0b1001, None),
            Some(48_000) => (0b1010, None),
            Some(96_000) => (0b1011, None),
            Some(rate) if rate.is_multiple_of(1000) && rate / 1000 <= 255 => {
                (0b1100, Some((rate / 1000, 8u32)))
            }
            Some(rate) if rate <= 65535 => (0b1101, Some((rate, 16))),
            Some(rate) if rate.is_multiple_of(10) && rate / 10 <= 65535 => {
                (0b1110, Some((rate / 10, 16)))
            }
            // フレームヘッダーで表現できないサンプルレートは STREAMINFO 参照で送る
            Some(_) => (0b0000, None),
        };

        // ビット深度 (RFC 9639 Section 9.1.4)
        let bit_depth_bits = match self.bits_per_sample {
            None => 0b000,
            Some(8) => 0b001,
            Some(12) => 0b010,
            Some(16) => 0b100,
            Some(20) => 0b101,
            Some(24) => 0b110,
            Some(32) => 0b111,
            // フレームヘッダーで表現できないビット深度は STREAMINFO 参照で送る
            Some(_) => 0b000,
        };

        writer.write_u32(FRAME_SYNC_CODE, 15);
        writer.write_u32(
            match self.blocking_strategy {
                BlockingStrategy::Fixed => 0,
                BlockingStrategy::Variable => 1,
            },
            1,
        );
        writer.write_u32(block_size_bits, 4);
        writer.write_u32(sample_rate_bits, 4);
        writer.write_u32(self.channel_assignment.to_bits(), 4);
        writer.write_u32(bit_depth_bits, 3);
        // 予約ビット (RFC 9639 Section 9.1.4)
        writer.write_u32(0, 1);

        encode_coded_number(writer, self.coded_number);
        if let Some(value) = uncommon_block_size {
            let bits = if self.block_size <= 256 { 8 } else { 16 };
            writer.write_u32(value, bits);
        }
        if let Some((value, bits)) = uncommon_sample_rate {
            writer.write_u32(value, bits);
        }

        // CRC-8 (RFC 9639 Section 9.1.8)
        debug_assert!(
            writer.is_byte_aligned(),
            "CRC-8 の位置はバイト境界 (実装バグ)"
        );
        let crc = crc8(&writer.as_bytes()[header_start..]);
        writer.write_u32(u32::from(crc), 8);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9639 Appendix D.1 のフレームヘッダー:
    /// FF F8 69 18 00 00 BF
    #[test]
    fn decode_frame_header_rfc9639_appendix_d1() {
        let data = [0xFF, 0xF8, 0x69, 0x18, 0x00, 0x00, 0xBF];
        let mut reader = BitReader::new(&data);
        let header = FrameHeader::decode(&mut reader).unwrap();
        assert_eq!(header.blocking_strategy, BlockingStrategy::Fixed);
        assert_eq!(header.block_size, 1);
        assert_eq!(header.sample_rate, Some(44_100));
        assert_eq!(header.channel_assignment, ChannelAssignment::Independent(2));
        assert_eq!(header.bits_per_sample, Some(16));
        assert_eq!(header.coded_number, 0);
    }

    /// RFC 9639 Appendix D.2 の第 1 フレームヘッダー:
    /// FF F8 69 98 00 0F 99 (side-right stereo, block size 16)
    #[test]
    fn decode_frame_header_rfc9639_appendix_d2() {
        let data = [0xFF, 0xF8, 0x69, 0x98, 0x00, 0x0F, 0x99];
        let mut reader = BitReader::new(&data);
        let header = FrameHeader::decode(&mut reader).unwrap();
        assert_eq!(header.blocking_strategy, BlockingStrategy::Fixed);
        assert_eq!(header.block_size, 16);
        assert_eq!(header.sample_rate, Some(44_100));
        assert_eq!(header.channel_assignment, ChannelAssignment::SideRight);
        assert_eq!(header.bits_per_sample, Some(16));
        assert_eq!(header.coded_number, 0);
    }

    #[test]
    fn decode_rejects_bad_sync_code() {
        let data = [0x00, 0xF8, 0x69, 0x18, 0x00, 0x00, 0xBF];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            FrameHeader::decode(&mut reader),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn decode_rejects_bad_crc() {
        let data = [0xFF, 0xF8, 0x69, 0x18, 0x00, 0x00, 0xC0];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            FrameHeader::decode(&mut reader),
            Err(ParseError::Invalid(
                DecodeError::FrameHeaderCrcMismatch { .. }
            ))
        ));
    }

    #[test]
    fn decode_needs_more_data_on_truncated_header() {
        let data = [0xFF, 0xF8, 0x69];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            FrameHeader::decode(&mut reader),
            Err(ParseError::NeedMoreData)
        ));
    }

    #[test]
    fn header_roundtrip_various() {
        let headers = [
            FrameHeader {
                blocking_strategy: BlockingStrategy::Fixed,
                block_size: 4096,
                sample_rate: Some(44_100),
                channel_assignment: ChannelAssignment::Independent(2),
                bits_per_sample: Some(16),
                coded_number: 0,
            },
            FrameHeader {
                blocking_strategy: BlockingStrategy::Variable,
                block_size: 1000,
                sample_rate: Some(48_000),
                channel_assignment: ChannelAssignment::MidSide,
                bits_per_sample: Some(24),
                coded_number: 51_000_000_000,
            },
            FrameHeader {
                blocking_strategy: BlockingStrategy::Fixed,
                block_size: 192,
                sample_rate: None,
                channel_assignment: ChannelAssignment::Independent(8),
                bits_per_sample: None,
                coded_number: 0x7FFF_FFFF,
            },
            FrameHeader {
                blocking_strategy: BlockingStrategy::Fixed,
                block_size: 16,
                sample_rate: Some(11_025 * 4), // uncommon: 44100 は定義済なので 44100*... ではなく 11025*4=44100 になってしまう
                channel_assignment: ChannelAssignment::LeftSide,
                bits_per_sample: Some(8),
                coded_number: 12345,
            },
        ];
        for header in headers {
            let mut writer = BitWriter::new();
            header.encode(&mut writer).unwrap();
            let bytes = writer.into_bytes();
            let mut reader = BitReader::new(&bytes);
            let decoded = FrameHeader::decode(&mut reader).unwrap();
            assert_eq!(decoded, header);
        }
    }

    #[test]
    fn header_roundtrip_uncommon_sample_rates() {
        // 8 bit kHz / 16 bit Hz / 16 bit Hz*10 の各 uncommon 形式
        for rate in [255_000u32, 12_345, 65_535, 655_350, 91_230] {
            let header = FrameHeader {
                blocking_strategy: BlockingStrategy::Fixed,
                block_size: 4096,
                sample_rate: Some(rate),
                channel_assignment: ChannelAssignment::Independent(1),
                bits_per_sample: Some(16),
                coded_number: 1,
            };
            let mut writer = BitWriter::new();
            header.encode(&mut writer).unwrap();
            let bytes = writer.into_bytes();
            let mut reader = BitReader::new(&bytes);
            let decoded = FrameHeader::decode(&mut reader).unwrap();
            assert_eq!(decoded.sample_rate, Some(rate), "rate {}", rate);
        }
    }

    /// RFC 9639 Section 9.1.5 の例: 51 billion のサンプル番号は
    /// 0xFE 0xAF 0x9F 0xB5 0xA3 0xB8 0x80 の 7 バイトで符号化される
    #[test]
    fn coded_number_rfc9639_example() {
        let mut writer = BitWriter::new();
        encode_coded_number(&mut writer, 51_000_000_000);
        assert_eq!(
            writer.into_bytes(),
            [0xFE, 0xAF, 0x9F, 0xB5, 0xA3, 0xB8, 0x80]
        );

        let data = [0xFE, 0xAF, 0x9F, 0xB5, 0xA3, 0xB8, 0x80];
        let mut reader = BitReader::new(&data);
        assert_eq!(decode_coded_number(&mut reader).unwrap(), 51_000_000_000);
    }

    #[test]
    fn coded_number_roundtrip_boundaries() {
        // 各バイト長の境界値
        let values = [
            0u64,
            0x7F,
            0x80,
            0x7FF,
            0x800,
            0xFFFF,
            0x1_0000,
            0x1F_FFFF,
            0x20_0000,
            0x3FF_FFFF,
            0x400_0000,
            0x7FFF_FFFF,
            0x8000_0000,
            0xF_FFFF_FFFF,
        ];
        for value in values {
            let mut writer = BitWriter::new();
            encode_coded_number(&mut writer, value);
            let bytes = writer.into_bytes();
            let mut reader = BitReader::new(&bytes);
            assert_eq!(
                decode_coded_number(&mut reader).unwrap(),
                value,
                "value {}",
                value
            );
        }
    }

    #[test]
    fn coded_number_rejects_invalid_leading_byte() {
        // 継続バイト 0b10xxxxxx が先頭に来るのは不正
        let data = [0x80];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            decode_coded_number(&mut reader),
            Err(ParseError::Invalid(_))
        ));
        // 0xFF も不正
        let data = [0xFF];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            decode_coded_number(&mut reader),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn coded_number_rejects_invalid_continuation_byte() {
        // 2 バイト形式の 2 バイト目が 0b11xxxxxx
        let data = [0xC2, 0xC0];
        let mut reader = BitReader::new(&data);
        assert!(matches!(
            decode_coded_number(&mut reader),
            Err(ParseError::Invalid(_))
        ));
    }
}
