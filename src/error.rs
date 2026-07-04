//! デコード / エンコードのエラー型

use alloc::string::String;
use core::fmt;

/// FLAC デコードエラー
///
/// データ不足は本エラーではなく `Option::None` (追加の feed 待ち) で表現するため、
/// 本エラーはフォーマット違反・チェックサム不一致など回復不能な状況のみを表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// ストリーム先頭の fLaC マーカーが不正 (RFC 9639 Section 6)
    InvalidStreamMarker { found: [u8; 4] },
    /// フォーマット違反のデータを検出した
    InvalidData(String),
    /// フレームヘッダーの CRC-8 が一致しない (RFC 9639 Section 9.1.8)
    FrameHeaderCrcMismatch { expected: u8, actual: u8 },
    /// フレームフッターの CRC-16 が一致しない (RFC 9639 Section 9.3)
    FrameCrcMismatch { expected: u16, actual: u16 },
    /// デコードした全サンプルの MD5 が STREAMINFO の値と一致しない (RFC 9639 Section 8.2)
    Md5Mismatch {
        expected: [u8; 16],
        actual: [u8; 16],
    },
    /// 入力終端 (finish 済み) なのにストリームが途中で終わっている
    TruncatedStream,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::InvalidStreamMarker { found } => {
                write!(
                    f,
                    "invalid stream marker: expected fLaC, found {:02x?} (RFC 9639 Section 6)",
                    found
                )
            }
            DecodeError::InvalidData(msg) => write!(f, "invalid data: {}", msg),
            DecodeError::FrameHeaderCrcMismatch { expected, actual } => {
                write!(
                    f,
                    "frame header CRC-8 mismatch: expected {:#04x}, actual {:#04x} (RFC 9639 Section 9.1.8)",
                    expected, actual
                )
            }
            DecodeError::FrameCrcMismatch { expected, actual } => {
                write!(
                    f,
                    "frame footer CRC-16 mismatch: expected {:#06x}, actual {:#06x} (RFC 9639 Section 9.3)",
                    expected, actual
                )
            }
            DecodeError::Md5Mismatch { expected, actual } => {
                write!(
                    f,
                    "MD5 checksum mismatch: expected {:02x?}, actual {:02x?} (RFC 9639 Section 8.2)",
                    expected, actual
                )
            }
            DecodeError::TruncatedStream => {
                write!(f, "stream is truncated")
            }
        }
    }
}

impl core::error::Error for DecodeError {}

/// FLAC エンコードエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// エンコーダー設定が不正
    InvalidConfig(String),
    /// サンプル値がビット深度で表現できる範囲の外
    SampleOutOfRange { value: i32, bits_per_sample: u8 },
    /// 投入されたサンプル数がチャンネル数の倍数でない
    UnalignedSamples { count: usize, channels: u8 },
    /// 合計サンプル数が STREAMINFO の 36 bit 上限を超えた (RFC 9639 Section 8.2)
    TooManySamples,
    /// メタデータブロックが不正 (サイズ超過・値域違反など)
    InvalidMetadata(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::InvalidConfig(msg) => write!(f, "invalid config: {}", msg),
            EncodeError::SampleOutOfRange {
                value,
                bits_per_sample,
            } => {
                write!(
                    f,
                    "sample value {} is out of range for {} bits per sample",
                    value, bits_per_sample
                )
            }
            EncodeError::UnalignedSamples { count, channels } => {
                write!(
                    f,
                    "sample count {} is not a multiple of channel count {}",
                    count, channels
                )
            }
            EncodeError::TooManySamples => {
                write!(
                    f,
                    "total sample count exceeds the 36-bit limit (RFC 9639 Section 8.2)"
                )
            }
            EncodeError::InvalidMetadata(msg) => write!(f, "invalid metadata: {}", msg),
        }
    }
}

impl core::error::Error for EncodeError {}

/// フレーム / メタデータをパースする内部処理のエラー
///
/// Sans I/O 設計では「データ不足 (追加の feed 待ち)」と「フォーマット違反」を
/// 区別する必要がある。前者はデコーダーが `Option::None` に変換し、後者は
/// `DecodeError` として利用者に返す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseError {
    /// データ不足 (入力終端でなければ追加の feed で解消し得る)
    NeedMoreData,
    /// フォーマット違反 (回復不能)
    Invalid(DecodeError),
}

impl From<crate::bit_reader::BitReadError> for ParseError {
    fn from(e: crate::bit_reader::BitReadError) -> Self {
        match e {
            crate::bit_reader::BitReadError::UnexpectedEof => ParseError::NeedMoreData,
        }
    }
}

impl From<DecodeError> for ParseError {
    fn from(e: DecodeError) -> Self {
        ParseError::Invalid(e)
    }
}
