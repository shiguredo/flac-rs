//! FLAC ファイルを WAV ファイルにデコードするサンプル
//!
//! ファイル全体をメモリに読み込まず、チャンクごとに `feed()` しながら
//! フレーム単位でデコード結果を書き出す (Sans I/O のストリーミング利用例)。

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use shiguredo_flac::decoder::StreamDecoder;

mod wav;

/// 1 回の読み込みサイズ
const CHUNK_SIZE: usize = 64 * 1024;

fn main() -> noargs::Result<()> {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = env!("CARGO_PKG_NAME");
    args.metadata_mut().app_description = env!("CARGO_PKG_DESCRIPTION");

    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    noargs::HELP_FLAG.take_help(&mut args);

    let input: PathBuf = noargs::arg("<INPUT>")
        .doc("入力 FLAC ファイル")
        .example("input.flac")
        .take(&mut args)
        .then(|a| a.value().parse())?;
    let output: PathBuf = noargs::arg("<OUTPUT>")
        .doc("出力 WAV ファイル")
        .example("output.wav")
        .take(&mut args)
        .then(|a| a.value().parse())?;

    if let Some(help) = args.finish()? {
        print!("{help}");
        return Ok(());
    }

    let mut file = File::open(&input)?;
    let mut decoder = StreamDecoder::new();
    let mut writer: Option<wav::WavWriter> = None;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut total_samples: u64 = 0;

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            decoder.finish();
        } else {
            decoder.feed(&buf[..n]);
        }

        // 読み込んだ分から取り出せるフレームを全て書き出す
        while let Some(frame) = decoder.decode_frame()? {
            let writer = match &mut writer {
                Some(writer) => writer,
                None => {
                    // 最初のフレームでフォーマットが確定する
                    writer.insert(wav::WavWriter::create(
                        &output,
                        u16::from(frame.channels),
                        frame.sample_rate,
                        u16::from(frame.bits_per_sample),
                    )?)
                }
            };
            writer.write_samples(&frame.samples)?;
            total_samples += u64::from(frame.header.block_size);
        }

        if n == 0 {
            break;
        }
    }

    let Some(writer) = writer else {
        return Err("FLAC stream contains no audio frames".into());
    };
    writer.finalize()?;

    let info = decoder
        .stream_info()
        .expect("デコードが成功していれば STREAMINFO は存在する");
    eprintln!(
        "decoded {} samples ({} channels, {} bits, {} Hz) into {}",
        total_samples,
        info.channels,
        info.bits_per_sample,
        info.sample_rate,
        output.display()
    );
    Ok(())
}
