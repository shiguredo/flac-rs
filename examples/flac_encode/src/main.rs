//! WAV ファイルを FLAC ファイルにエンコードするサンプル
//!
//! WAV の PCM データをブロックずつ読み込みながら `push_samples()` で投入する
//! (Sans I/O のストリーミング利用例)。

use std::path::PathBuf;

use shiguredo_flac::encoder::{StreamEncoder, StreamEncoderConfig};
use shiguredo_flac::metadata::MetadataBlock;
use shiguredo_flac::vorbis_comment::{VorbisComment, VorbisCommentField};

mod wav;

/// 1 回の読み込みサンプル数
const CHUNK_SAMPLES: usize = 64 * 1024;

fn main() -> noargs::Result<()> {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = env!("CARGO_PKG_NAME");
    args.metadata_mut().app_description = env!("CARGO_PKG_DESCRIPTION");

    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    noargs::HELP_FLAG.take_help(&mut args);

    let block_size: u16 = noargs::opt("block-size")
        .doc("ブロックサイズ (16-65535 サンプル)")
        .ty("N")
        .default("4096")
        .take(&mut args)
        .then(|o| o.value().parse())?;
    let max_lpc_order: u8 = noargs::opt("max-lpc-order")
        .doc("LPC の最大次数 (0-32、0 で固定予測のみ)")
        .ty("N")
        .default("8")
        .take(&mut args)
        .then(|o| o.value().parse())?;
    let title: Option<String> = noargs::opt("title")
        .doc("VORBIS_COMMENT の TITLE フィールド")
        .ty("TEXT")
        .take(&mut args)
        .present_and_then(|o| o.value().parse())?;
    let input: PathBuf = noargs::arg("<INPUT>")
        .doc("入力 WAV ファイル")
        .example("input.wav")
        .take(&mut args)
        .then(|a| a.value().parse())?;
    let output: PathBuf = noargs::arg("<OUTPUT>")
        .doc("出力 FLAC ファイル")
        .example("output.flac")
        .take(&mut args)
        .then(|a| a.value().parse())?;

    if let Some(help) = args.finish()? {
        print!("{help}");
        return Ok(());
    }

    let mut reader = wav::WavReader::open(&input)?;

    // タイトル指定があれば VORBIS_COMMENT として埋め込む
    let mut metadata = Vec::new();
    if let Some(title) = title {
        metadata.push(MetadataBlock::VorbisComment(VorbisComment {
            vendor: format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
            fields: vec![VorbisCommentField {
                name: "TITLE".to_string(),
                value: title,
            }],
        }));
    }

    let config = StreamEncoderConfig {
        sample_rate: reader.sample_rate,
        channels: u8::try_from(reader.channels)
            .map_err(|_| format!("unsupported channel count {}", reader.channels))?,
        bits_per_sample: reader.bits_per_sample as u8,
        block_size,
        max_lpc_order,
        metadata,
        ..StreamEncoderConfig::default()
    };
    let mut encoder = StreamEncoder::new(config)?;

    // WAV の PCM をブロックずつ読み込みながらエンコードする
    let chunk = CHUNK_SAMPLES - CHUNK_SAMPLES % usize::from(reader.channels);
    let mut total_samples: u64 = 0;
    loop {
        let samples = reader.read_samples(chunk)?;
        if samples.is_empty() {
            break;
        }
        total_samples += (samples.len() / usize::from(reader.channels)) as u64;
        encoder.push_samples(&samples)?;
    }
    let flac_bytes = encoder.finish()?;
    std::fs::write(&output, &flac_bytes)?;

    eprintln!(
        "encoded {} samples ({} channels, {} bits, {} Hz) into {} ({} bytes)",
        total_samples,
        reader.channels,
        reader.bits_per_sample,
        reader.sample_rate,
        output.display(),
        flac_bytes.len()
    );
    Ok(())
}
