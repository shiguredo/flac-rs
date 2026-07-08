//! # shiguredo_flac
//!
//! 依存なしの FLAC (Free Lossless Audio Codec, RFC 9639) ライブラリ (Sans I/O)
//!
//! ## 特徴
//!
//! - **依存なし**: `core` / `alloc` のみ (no_std 対応)
//! - **Sans I/O**: I/O を完全に分離した設計
//! - **RFC 9639 準拠**: 性能より正しさ・堅牢性を優先
//! - **ロスレス保証**: エンコード → デコードで元のサンプル列と完全一致
//!
//! ## 使い方
//!
//! ### エンコード (PCM から FLAC へ)
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
//! // チャンネルインターリーブ済み (L, R, L, R, ...) のサンプルを投入する
//! encoder.push_samples(&[100, -100, 200, -200])?;
//! let flac_bytes = encoder.finish()?;
//! // flac_bytes を書き出す...
//! # Ok(())
//! # }
//! ```
//!
//! ### デコード (FLAC から PCM へ)
//!
//! ```rust,no_run
//! use shiguredo_flac::decoder::StreamDecoder;
//!
//! # fn main() -> Result<(), shiguredo_flac::error::DecodeError> {
//! let mut decoder = StreamDecoder::new();
//! // 受信データを feed し、入力終端で finish を呼ぶ
//! # let received: &[u8] = &[];
//! decoder.feed(received);
//! decoder.finish();
//!
//! while let Some(frame) = decoder.decode_frame()? {
//!     // frame.samples はチャンネルインターリーブ済みのサンプル列
//! }
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(test), no_std)]
extern crate alloc;

mod bit_reader;
mod bit_writer;
mod crc;
pub mod cuesheet;
pub mod decoder;
pub mod encoder;
pub mod error;
mod fixed;
pub mod frame;
mod lpc;
mod md5;
pub mod metadata;
pub mod picture;
mod rice;
mod subframe;
pub mod vorbis_comment;
