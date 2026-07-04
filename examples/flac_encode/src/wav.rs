//! WAV (RIFF) ファイルの読み込み
//!
//! PCM (フォーマットタグ 1) の 8 / 16 / 24 / 32 bit のみをサポートする。
//! 8 bit は unsigned、それ以外は signed little-endian として解釈する。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// WAV ファイルリーダー
pub struct WavReader {
    file: File,
    /// チャンネル数
    pub channels: u16,
    /// サンプルレート (Hz)
    pub sample_rate: u32,
    /// サンプルあたりのビット数
    pub bits_per_sample: u16,
    /// data チャンクの残りバイト数
    data_remaining: u64,
}

/// フォーマット違反を `io::Error` にする
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl WavReader {
    /// WAV ファイルを開き、fmt チャンクを解釈して data チャンクの先頭まで進める
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;

        let mut riff = [0u8; 12];
        file.read_exact(&mut riff)?;
        if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
            return Err(invalid("not a RIFF/WAVE file"));
        }

        // fmt チャンクを見つけてから data チャンクまで読み進める
        let mut format: Option<(u16, u32, u16)> = None;
        loop {
            let mut header = [0u8; 8];
            file.read_exact(&mut header)?;
            let chunk_id: [u8; 4] = header[0..4].try_into().expect("4 バイト固定");
            let chunk_size = u64::from(u32::from_le_bytes(
                header[4..8].try_into().expect("4 バイト固定"),
            ));
            match &chunk_id {
                b"fmt " => {
                    if chunk_size < 16 {
                        return Err(invalid("fmt chunk is too short"));
                    }
                    let mut fmt = [0u8; 16];
                    file.read_exact(&mut fmt)?;
                    let audio_format = u16::from_le_bytes([fmt[0], fmt[1]]);
                    if audio_format != 1 {
                        return Err(invalid("only PCM (format tag 1) WAV is supported"));
                    }
                    let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                    let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                    let bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);
                    if !matches!(bits_per_sample, 8 | 16 | 24 | 32) {
                        return Err(invalid("unsupported bit depth (supported: 8, 16, 24, 32)"));
                    }
                    format = Some((channels, sample_rate, bits_per_sample));
                    // fmt チャンクの拡張部分を読み飛ばす (奇数サイズはパディング)
                    let rest = chunk_size - 16 + (chunk_size % 2);
                    file.seek(SeekFrom::Current(rest as i64))?;
                }
                b"data" => {
                    let Some((channels, sample_rate, bits_per_sample)) = format else {
                        return Err(invalid("data chunk appears before fmt chunk"));
                    };
                    return Ok(Self {
                        file,
                        channels,
                        sample_rate,
                        bits_per_sample,
                        data_remaining: chunk_size,
                    });
                }
                _ => {
                    // 他のチャンクは読み飛ばす (奇数サイズはパディング)
                    file.seek(SeekFrom::Current((chunk_size + chunk_size % 2) as i64))?;
                }
            }
        }
    }

    /// インターリーブ済みサンプルを最大 `max_samples` 個読み込む
    ///
    /// data チャンクの終端に達したら空のベクタを返す。
    pub fn read_samples(&mut self, max_samples: usize) -> io::Result<Vec<i32>> {
        let bytes_per_sample = u64::from(self.bits_per_sample / 8);
        let to_read = (max_samples as u64 * bytes_per_sample).min(self.data_remaining);
        // サンプル境界に切り捨てる
        let to_read = to_read - to_read % bytes_per_sample;
        if to_read == 0 {
            return Ok(Vec::new());
        }

        let mut data = vec![0u8; to_read as usize];
        self.file.read_exact(&mut data)?;
        self.data_remaining -= to_read;

        let mut samples = Vec::new();
        for chunk in data.chunks_exact(bytes_per_sample as usize) {
            let sample = match self.bits_per_sample {
                // 8 bit WAV は unsigned (128 がゼロ点)
                8 => i32::from(chunk[0]) - 128,
                16 => i32::from(i16::from_le_bytes([chunk[0], chunk[1]])),
                24 => {
                    // 24 bit を符号拡張する
                    i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]) << 8 >> 8
                }
                _ => i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            };
            samples.push(sample);
        }
        Ok(samples)
    }
}
