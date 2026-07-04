//! WAV (RIFF) の一括読み書き
//!
//! 本家 flac コマンドと PCM をやり取りするための比較ツール用実装。
//! 本家 flac -d は 24 bit 出力を WAVE_FORMAT_EXTENSIBLE (フォーマットタグ
//! 0xFFFE) で書き出すため、examples の WavReader と異なり 0xFFFE を受理する。
//! 同じ理由で WAV ファイル同士のバイト比較は成立せず、比較は常に data
//! チャンクから復元したサンプル列で行う。
//!
//! ビット深度は比較ケースで使う 16 / 24 bit のみサポートする。

use std::io;
use std::path::Path;

/// 読み込んだ WAV の内容
pub struct WavData {
    /// チャンネル数
    pub channels: u16,
    /// サンプルレート (Hz)
    pub sample_rate: u32,
    /// サンプルあたりのビット数
    pub bits_per_sample: u16,
    /// チャンネルインターリーブ済みサンプル
    pub samples: Vec<i32>,
}

/// フォーマット違反を `io::Error` にする
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// WAV ファイル全体を読み込む
pub fn read(path: &Path) -> io::Result<WavData> {
    let data = std::fs::read(path)?;
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(invalid("not a RIFF/WAVE file"));
    }

    // fmt チャンクを見つけてから data チャンクまで走査する
    let mut pos = 12usize;
    let mut format: Option<(u16, u32, u16)> = None;
    loop {
        if pos + 8 > data.len() {
            return Err(invalid("data chunk not found"));
        }
        let chunk_id = &data[pos..pos + 4];
        let chunk_size =
            u32::from_le_bytes(data[pos + 4..pos + 8].try_into().expect("4 バイト固定")) as usize;
        pos += 8;
        if pos + chunk_size > data.len() {
            return Err(invalid("chunk size exceeds file size"));
        }
        let chunk = &data[pos..pos + chunk_size];
        match chunk_id {
            b"fmt " => format = Some(parse_fmt(chunk)?),
            b"data" => {
                let Some((channels, sample_rate, bits_per_sample)) = format else {
                    return Err(invalid("data chunk appears before fmt chunk"));
                };
                return Ok(WavData {
                    channels,
                    sample_rate,
                    bits_per_sample,
                    samples: decode_pcm(chunk, bits_per_sample)?,
                });
            }
            _ => {}
        }
        // 奇数サイズのチャンクは 1 バイトのパディングを挟む
        pos += chunk_size + chunk_size % 2;
    }
}

/// fmt チャンクからチャンネル数・サンプルレート・ビット深度を取り出す
fn parse_fmt(chunk: &[u8]) -> io::Result<(u16, u32, u16)> {
    if chunk.len() < 16 {
        return Err(invalid("fmt chunk is too short"));
    }
    let audio_format = u16::from_le_bytes([chunk[0], chunk[1]]);
    let channels = u16::from_le_bytes([chunk[2], chunk[3]]);
    let sample_rate = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
    let bits_per_sample = u16::from_le_bytes([chunk[14], chunk[15]]);
    match audio_format {
        // WAVE_FORMAT_PCM
        1 => {}
        // WAVE_FORMAT_EXTENSIBLE。SubFormat GUID の先頭 4 バイトが 1 なら PCM
        0xFFFE => {
            if chunk.len() < 40 {
                return Err(invalid("fmt chunk is too short for WAVE_FORMAT_EXTENSIBLE"));
            }
            let sub_format = u32::from_le_bytes([chunk[24], chunk[25], chunk[26], chunk[27]]);
            if sub_format != 1 {
                return Err(invalid("only the PCM subformat is supported"));
            }
        }
        _ => return Err(invalid("only PCM WAV is supported")),
    }
    if !matches!(bits_per_sample, 16 | 24) {
        return Err(invalid("unsupported bit depth (supported: 16, 24)"));
    }
    Ok((channels, sample_rate, bits_per_sample))
}

/// data チャンクのバイト列をサンプル列に変換する
fn decode_pcm(data: &[u8], bits_per_sample: u16) -> io::Result<Vec<i32>> {
    let bytes_per_sample = usize::from(bits_per_sample / 8);
    if !data.len().is_multiple_of(bytes_per_sample) {
        return Err(invalid("data chunk is not aligned to the sample size"));
    }
    let mut samples = Vec::new();
    for chunk in data.chunks_exact(bytes_per_sample) {
        let sample = match bits_per_sample {
            16 => i32::from(i16::from_le_bytes([chunk[0], chunk[1]])),
            // 24 bit を符号拡張する
            _ => i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]) << 8 >> 8,
        };
        samples.push(sample);
    }
    Ok(samples)
}

/// WAV ファイルを書き出す
///
/// 総サンプル数が既知なので、サイズフィールドは最初から確定値で書く。
pub fn write(
    path: &Path,
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    samples: &[i32],
) -> io::Result<()> {
    if !matches!(bits_per_sample, 16 | 24) {
        return Err(invalid("unsupported bit depth (supported: 16, 24)"));
    }
    let bytes_per_sample = bits_per_sample / 8;
    let data_bytes = samples.len() * usize::from(bytes_per_sample);
    // RIFF チャンクサイズ: WAVE (4) + fmt チャンク (8+16) + data ヘッダー (8) + データ
    let riff_size = u32::try_from(4 + 24 + 8 + data_bytes)
        .map_err(|_| invalid("audio data exceeds the 4 GiB WAV size limit"))?;
    let block_align = channels * bytes_per_sample;
    let byte_rate = sample_rate * u32::from(block_align);

    let mut out = Vec::with_capacity(44 + data_bytes);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // WAVE_FORMAT_PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for &sample in samples {
        match bytes_per_sample {
            2 => out.extend_from_slice(&(sample as i16).to_le_bytes()),
            _ => out.extend_from_slice(&sample.to_le_bytes()[..3]),
        }
    }
    std::fs::write(path, &out)
}
