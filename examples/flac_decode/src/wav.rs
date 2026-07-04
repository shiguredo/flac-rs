//! WAV (RIFF) ファイルの書き出し
//!
//! PCM (フォーマットタグ 1) の 8 / 16 / 24 / 32 bit のみをサポートする。
//! 8 bit は unsigned、それ以外は signed little-endian で格納する。

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

/// WAV ファイルライター
///
/// 総サンプル数はストリーミングデコードの完了まで分からないため、
/// ヘッダーのサイズフィールドは `finalize()` で書き戻す。
pub struct WavWriter {
    file: File,
    bytes_per_sample: u16,
    /// data チャンクに書き込んだバイト数
    data_bytes: u64,
}

impl WavWriter {
    /// WAV ファイルを作成してヘッダーを書き込む
    pub fn create<P: AsRef<Path>>(
        path: P,
        channels: u16,
        sample_rate: u32,
        bits_per_sample: u16,
    ) -> io::Result<Self> {
        if !matches!(bits_per_sample, 8 | 16 | 24 | 32) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "unsupported bit depth {} for WAV output (supported: 8, 16, 24, 32)",
                    bits_per_sample
                ),
            ));
        }
        let mut file = File::create(path)?;
        let bytes_per_sample = bits_per_sample / 8;
        let block_align = channels * bytes_per_sample;
        let byte_rate = sample_rate * u32::from(block_align);

        // RIFF ヘッダーと fmt チャンク。サイズフィールドは finalize() で確定する
        file.write_all(b"RIFF")?;
        file.write_all(&0u32.to_le_bytes())?;
        file.write_all(b"WAVE")?;
        file.write_all(b"fmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?; // PCM
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&sample_rate.to_le_bytes())?;
        file.write_all(&byte_rate.to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&bits_per_sample.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&0u32.to_le_bytes())?;

        Ok(Self {
            file,
            bytes_per_sample,
            data_bytes: 0,
        })
    }

    /// インターリーブ済みサンプルを書き込む
    pub fn write_samples(&mut self, samples: &[i32]) -> io::Result<()> {
        let mut data = Vec::new();
        for &sample in samples {
            match self.bytes_per_sample {
                // 8 bit WAV は unsigned (128 がゼロ点)
                1 => data.push((sample + 128) as u8),
                2 => data.extend_from_slice(&(sample as i16).to_le_bytes()),
                3 => data.extend_from_slice(&sample.to_le_bytes()[..3]),
                _ => data.extend_from_slice(&sample.to_le_bytes()),
            }
        }
        self.file.write_all(&data)?;
        self.data_bytes += data.len() as u64;
        Ok(())
    }

    /// サイズフィールドを確定してファイルを閉じる
    pub fn finalize(mut self) -> io::Result<()> {
        // RIFF チャンクサイズ: WAVE (4) + fmt チャンク (8+16) + data ヘッダー (8) + データ
        let riff_size = 4 + 24 + 8 + self.data_bytes;
        let (riff_size, data_size) = (u32::try_from(riff_size), u32::try_from(self.data_bytes));
        let (Ok(riff_size), Ok(data_size)) = (riff_size, data_size) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "audio data exceeds the 4 GiB WAV size limit",
            ));
        };
        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&riff_size.to_le_bytes())?;
        self.file.seek(SeekFrom::Start(40))?;
        self.file.write_all(&data_size.to_le_bytes())?;
        self.file.flush()
    }
}
