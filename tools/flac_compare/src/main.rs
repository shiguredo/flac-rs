//! 本家 flac コマンドとの比較ツール
//!
//! flac-rs のエンコード / デコードを本家 flac (リファレンス実装) と突き合わせ、
//! 以下を 1 コマンドで報告する。
//!
//! - 相互運用: flac-rs encode → 本家 decode / flac -t、本家 encode → flac-rs decode
//!   (PCM はサンプル列で比較する。本家 flac -d は 24 bit 出力を
//!   WAVE_FORMAT_EXTENSIBLE で書くため、WAV のバイト比較は成立しない)
//! - 圧縮率: メタデータブロックを除いたフレームデータのバイト数で比較する
//!   (本家はデフォルトで SEEKTABLE / VORBIS_COMMENT / PADDING 約 8.3 KB を書く)
//! - 速度: CLI end-to-end の実行時間 (examples の flac_encode / flac_decode と
//!   本家コマンドをプロセス起動込みの同条件で計測する)
//!
//! 本家 flac は --flac-bin、環境変数 FLAC_BIN、PATH の順で探す。
//! 使い方: make compare (または cargo run --release -p flac_compare)

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use shiguredo_flac::decoder::decode;
use shiguredo_flac::encoder::{StreamEncoderConfig, encode};

mod signal;
mod wav;

/// 速度計測の実行回数 (最小値を採用する)
const SPEED_RUNS: u32 = 3;

fn main() -> noargs::Result<()> {
    let mut args = noargs::raw_args();
    args.metadata_mut().app_name = env!("CARGO_PKG_NAME");
    args.metadata_mut().app_description = env!("CARGO_PKG_DESCRIPTION");

    if noargs::VERSION_FLAG.take(&mut args).is_present() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    noargs::HELP_FLAG.take_help(&mut args);

    let flac_bin: Option<PathBuf> = noargs::opt("flac-bin")
        .doc("本家 flac コマンドのパス (省略時は FLAC_BIN 環境変数、次いで PATH から探す)")
        .ty("PATH")
        .take(&mut args)
        .present_and_then(|o| o.value().parse())?;
    let skip_speed = noargs::flag("skip-speed")
        .doc("速度計測をスキップする")
        .take(&mut args)
        .is_present();
    let seconds: u32 = noargs::opt("seconds")
        .doc("速度計測に使う信号の長さ (秒)")
        .ty("N")
        .default("60")
        .take(&mut args)
        .then(|o| o.value().parse())?;

    if let Some(help) = args.finish()? {
        print!("{help}");
        return Ok(());
    }

    let reference = ReferenceFlac::locate(flac_bin)?;
    println!(
        "reference: {} ({})",
        reference.bin.display(),
        reference.version()?
    );

    let work_dir = work_dir()?;
    std::fs::create_dir_all(&work_dir)?;
    println!("work dir: {}", work_dir.display());

    // 相互運用と圧縮率
    let cases = build_cases();
    let reports: Vec<CaseReport> = cases
        .iter()
        .map(|case| run_case(case, &reference, &work_dir))
        .collect();
    print_interop(&reports);
    print_compression(&reports);

    // 速度
    let mut speed_failed = false;
    if !skip_speed {
        match run_speed(&reference, &work_dir, seconds) {
            Ok(speed) => print_speed(&speed, seconds),
            Err(e) => {
                eprintln!("speed measurement failed: {e}");
                speed_failed = true;
            }
        }
    }

    // 集計 (相互運用のチェックは 1 ケースにつき 3 つ)
    let failures: usize = reports.iter().map(CaseReport::failure_count).sum();
    let checks = reports.len() * 3;
    println!();
    if failures == 0 {
        println!("result: all {checks} interoperability checks passed");
    } else {
        println!("result: {failures} of {checks} interoperability checks FAILED");
    }
    if failures > 0 || speed_failed {
        std::process::exit(1);
    }
    Ok(())
}

/// 比較対象の信号ケース
struct SignalCase {
    /// 信号名 (作業ファイル名にも使うため一意にする)
    name: &'static str,
    /// チャンネル数
    channels: u8,
    /// サンプルレート (Hz)
    sample_rate: u32,
    /// ビット深度
    bits_per_sample: u8,
    /// チャンネルインターリーブ済みサンプル
    samples: Vec<i32>,
    /// 圧縮率の表に含めるか (フォーマットバリエーションは相互運用のみ確認する)
    in_compression_table: bool,
}

impl SignalCase {
    /// "tonal (16bit/44.1kHz/2ch)" 形式の表示ラベルを返す
    fn label(&self) -> String {
        format!(
            "{} ({}bit/{}kHz/{}ch)",
            self.name,
            self.bits_per_sample,
            f64::from(self.sample_rate) / 1000.0,
            self.channels
        )
    }
}

/// 比較する信号ケース (各 5 秒) を構築する
fn build_cases() -> Vec<SignalCase> {
    /// 16 bit / 44.1 kHz ケースの信号長 (5 秒)
    const FRAMES: usize = 44_100 * 5;

    let stereo_16bit = |name: &'static str, samples: Vec<i32>| SignalCase {
        name,
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        samples,
        in_compression_table: true,
    };
    vec![
        stereo_16bit("silence", signal::silence(FRAMES)),
        stereo_16bit("tonal", signal::tonal(FRAMES, 1)),
        stereo_16bit("square", signal::square(FRAMES)),
        stereo_16bit("sweep", signal::sweep(FRAMES)),
        stereo_16bit("noise", signal::noise(FRAMES)),
        stereo_16bit("quiet", signal::quiet(FRAMES)),
        stereo_16bit("impulse", signal::impulse(FRAMES)),
        stereo_16bit("wasted", signal::wasted(FRAMES)),
        stereo_16bit("mixed", signal::mixed(FRAMES)),
        // フォーマットバリエーション (相互運用のみ確認する)
        SignalCase {
            name: "tonal_24bit",
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 24,
            samples: signal::tonal(48_000 * 5, 256),
            in_compression_table: false,
        },
        SignalCase {
            name: "tonal_mono",
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            samples: signal::tonal_mono(FRAMES),
            in_compression_table: false,
        },
    ]
}

/// 本家 flac コマンドの呼び出し
struct ReferenceFlac {
    /// flac コマンドのパス
    bin: PathBuf,
}

impl ReferenceFlac {
    /// 本家 flac コマンドを --flac-bin 引数、FLAC_BIN 環境変数、PATH の順で探す
    fn locate(explicit: Option<PathBuf>) -> Result<Self, String> {
        if let Some(bin) = explicit.or_else(|| std::env::var_os("FLAC_BIN").map(PathBuf::from)) {
            if !bin.is_file() {
                return Err(format!("reference flac not found at {}", bin.display()));
            }
            return Ok(Self { bin });
        }
        for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            let candidate = dir.join("flac");
            if candidate.is_file() {
                return Ok(Self { bin: candidate });
            }
        }
        Err(
            "reference flac command not found; specify --flac-bin, set FLAC_BIN, or add flac to PATH"
                .to_string(),
        )
    }

    /// flac --version の出力 (1 行目) を返す
    fn version(&self) -> Result<String, String> {
        let output = Command::new(&self.bin)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("failed to run {}: {e}", self.bin.display()))?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("unknown")
            .to_string())
    }

    /// --totally-silent 付きのコマンドを組み立てる
    fn command(&self) -> Command {
        let mut command = Command::new(&self.bin);
        command.arg("--totally-silent").stdin(Stdio::null());
        command
    }

    /// 指定レベルでエンコードする
    fn encode(&self, level: &str, wav_path: &Path, out: &Path) -> Result<(), String> {
        let mut command = self.command();
        command
            .arg(level)
            .arg("-f")
            .arg("-o")
            .arg(out)
            .arg(wav_path);
        run(command)
    }

    /// WAV にデコードする
    fn decode(&self, flac_path: &Path, out: &Path) -> Result<(), String> {
        let mut command = self.command();
        command
            .arg("-d")
            .arg("-f")
            .arg("-o")
            .arg(out)
            .arg(flac_path);
        run(command)
    }

    /// flac -t (MD5 込みのデコード検証) を実行する
    fn test(&self, flac_path: &Path) -> Result<(), String> {
        let mut command = self.command();
        command.arg("-t").arg(flac_path);
        run(command)
    }
}

/// コマンドを実行し、失敗したら stderr を含むエラーを返す
fn run(mut command: Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().to_string();
    let output = command
        .output()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// 作業ディレクトリ (target/flac_compare) を返す
///
/// 実行バイナリは target/<profile>/flac_compare にあるため、2 つ上が target になる。
fn work_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to locate the current executable: {e}"))?;
    let target = exe
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "failed to locate the target directory".to_string())?;
    Ok(target.join("flac_compare"))
}

/// フレームデータのバイト数 (flac-rs / 本家 -5 / 本家 -8)
type FrameBytes = (usize, usize, usize);

/// 1 ケースの比較結果
struct CaseReport {
    /// 表示ラベル (信号名 + フォーマット)
    label: String,
    /// 圧縮率の表に含めるか
    in_compression_table: bool,
    /// flac-rs encode → 本家 decode の PCM 一致
    rs_to_ref: Result<(), String>,
    /// flac-rs encode 出力の flac -t (MD5 込み検証)
    flac_t: Result<(), String>,
    /// 本家 encode (-5) → flac-rs decode の PCM 一致
    ref_to_rs: Result<(), String>,
    /// フレームデータのバイト数
    frame_bytes: Option<FrameBytes>,
}

impl CaseReport {
    /// 失敗した相互運用チェックの数を返す
    fn failure_count(&self) -> usize {
        [&self.rs_to_ref, &self.flac_t, &self.ref_to_rs]
            .iter()
            .filter(|result| result.is_err())
            .count()
    }
}

/// 1 ケースの相互運用と圧縮率を確認する
fn run_case(case: &SignalCase, reference: &ReferenceFlac, work_dir: &Path) -> CaseReport {
    // flac-rs でエンコードする。ここでの失敗は比較結果ではなく実装バグ
    let config = StreamEncoderConfig {
        sample_rate: case.sample_rate,
        channels: case.channels,
        bits_per_sample: case.bits_per_sample,
        ..StreamEncoderConfig::default()
    };
    let rs_flac = encode(config, &case.samples)
        .expect("flac-rs のエンコードは成功するはず (失敗したら実装バグ)");
    let rs_flac_path = work_dir.join(format!("{}_rs.flac", case.name));

    let (rs_to_ref, flac_t) = match std::fs::write(&rs_flac_path, &rs_flac) {
        Ok(()) => (
            check_rs_to_ref(case, reference, work_dir, &rs_flac_path),
            reference.test(&rs_flac_path),
        ),
        Err(e) => {
            let error = format!("failed to write {}: {e}", rs_flac_path.display());
            (Err(error.clone()), Err(error))
        }
    };
    let (ref_to_rs, frame_bytes) = check_ref_encode(case, reference, work_dir, &rs_flac);

    CaseReport {
        label: case.label(),
        in_compression_table: case.in_compression_table,
        rs_to_ref,
        flac_t,
        ref_to_rs,
        frame_bytes,
    }
}

/// flac-rs の出力を本家がデコードでき、PCM が一致することを確認する
fn check_rs_to_ref(
    case: &SignalCase,
    reference: &ReferenceFlac,
    work_dir: &Path,
    rs_flac_path: &Path,
) -> Result<(), String> {
    let out_wav = work_dir.join(format!("{}_rs_decoded.wav", case.name));
    reference.decode(rs_flac_path, &out_wav)?;
    let decoded =
        wav::read(&out_wav).map_err(|e| format!("failed to read the decoded WAV: {e}"))?;
    if decoded.channels != u16::from(case.channels) {
        return Err(format!(
            "channel count mismatch: expected {}, got {}",
            case.channels, decoded.channels
        ));
    }
    if decoded.bits_per_sample != u16::from(case.bits_per_sample) {
        return Err(format!(
            "bit depth mismatch: expected {}, got {}",
            case.bits_per_sample, decoded.bits_per_sample
        ));
    }
    if decoded.sample_rate != case.sample_rate {
        return Err(format!(
            "sample rate mismatch: expected {}, got {}",
            case.sample_rate, decoded.sample_rate
        ));
    }
    compare_pcm(&case.samples, &decoded.samples)
}

/// 本家エンコード出力の flac-rs デコードと、フレームデータサイズを確認する
///
/// 戻り値は (本家 -5 出力の flac-rs デコード結果, フレームデータのバイト数)。
fn check_ref_encode(
    case: &SignalCase,
    reference: &ReferenceFlac,
    work_dir: &Path,
    rs_flac: &[u8],
) -> (Result<(), String>, Option<FrameBytes>) {
    match prepare_ref_encode(case, reference, work_dir, rs_flac) {
        Ok((ref5_data, sizes)) => (decode_and_compare(case, &ref5_data), Some(sizes)),
        Err(e) => (Err(e), None),
    }
}

/// 元信号を本家で -5 / -8 エンコードし、-5 の出力とフレームデータサイズを返す
fn prepare_ref_encode(
    case: &SignalCase,
    reference: &ReferenceFlac,
    work_dir: &Path,
    rs_flac: &[u8],
) -> Result<(Vec<u8>, FrameBytes), String> {
    let src_wav = work_dir.join(format!("{}.wav", case.name));
    wav::write(
        &src_wav,
        u16::from(case.channels),
        case.sample_rate,
        u16::from(case.bits_per_sample),
        &case.samples,
    )
    .map_err(|e| format!("failed to write the source WAV: {e}"))?;
    let ref5_path = work_dir.join(format!("{}_ref5.flac", case.name));
    let ref8_path = work_dir.join(format!("{}_ref8.flac", case.name));
    reference.encode("-5", &src_wav, &ref5_path)?;
    reference.encode("-8", &src_wav, &ref8_path)?;

    let ref5_data = std::fs::read(&ref5_path)
        .map_err(|e| format!("failed to read {}: {e}", ref5_path.display()))?;
    let ref8_data = std::fs::read(&ref8_path)
        .map_err(|e| format!("failed to read {}: {e}", ref8_path.display()))?;
    let sizes = (
        frame_data_size(rs_flac)?,
        frame_data_size(&ref5_data)?,
        frame_data_size(&ref8_data)?,
    );
    Ok((ref5_data, sizes))
}

/// 本家のエンコード出力を flac-rs でデコードして PCM を比較する
fn decode_and_compare(case: &SignalCase, ref_flac: &[u8]) -> Result<(), String> {
    let decoded = decode(ref_flac)
        .map_err(|e| format!("flac-rs failed to decode the reference output: {e}"))?;
    if decoded.channels != case.channels {
        return Err(format!(
            "channel count mismatch: expected {}, got {}",
            case.channels, decoded.channels
        ));
    }
    if decoded.bits_per_sample != case.bits_per_sample {
        return Err(format!(
            "bit depth mismatch: expected {}, got {}",
            case.bits_per_sample, decoded.bits_per_sample
        ));
    }
    if decoded.sample_rate != case.sample_rate {
        return Err(format!(
            "sample rate mismatch: expected {}, got {}",
            case.sample_rate, decoded.sample_rate
        ));
    }
    compare_pcm(&case.samples, &decoded.samples)
}

/// サンプル列が完全一致することを確認する
fn compare_pcm(expected: &[i32], actual: &[i32]) -> Result<(), String> {
    if expected.len() != actual.len() {
        return Err(format!(
            "sample count mismatch: expected {}, got {}",
            expected.len(),
            actual.len()
        ));
    }
    for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
        if e != a {
            return Err(format!("sample {i} mismatch: expected {e}, got {a}"));
        }
    }
    Ok(())
}

/// FLAC ストリームのメタデータ部分を除いたフレームデータのバイト数を返す
///
/// 本家 flac はデフォルトで SEEKTABLE / VORBIS_COMMENT / PADDING (約 8.3 KB) を
/// 書き込むため、ファイルサイズ同士の比較では圧縮性能を評価できない。
/// メタデータブロックヘッダーは 1 バイトの (last フラグ | ブロックタイプ) と
/// 3 バイトのビッグエンディアン長 (RFC 9639 Section 8.1)。
fn frame_data_size(data: &[u8]) -> Result<usize, String> {
    if data.len() < 4 || &data[0..4] != b"fLaC" {
        return Err("not a FLAC stream".to_string());
    }
    let mut pos = 4usize;
    loop {
        if pos + 4 > data.len() {
            return Err("metadata block header is truncated".to_string());
        }
        let last = data[pos] & 0x80 != 0;
        let length = usize::from(data[pos + 1]) << 16
            | usize::from(data[pos + 2]) << 8
            | usize::from(data[pos + 3]);
        pos += 4 + length;
        if pos > data.len() {
            return Err("metadata block length exceeds the stream size".to_string());
        }
        if last {
            return Ok(data.len() - pos);
        }
    }
}

/// 相互運用の結果表を出力する
fn print_interop(reports: &[CaseReport]) {
    println!();
    println!("== interoperability ==");
    println!(
        "{:<30} {:<12} {:<8} {:<12}",
        "case", "rs->ref pcm", "flac -t", "ref->rs pcm"
    );
    for report in reports {
        println!(
            "{:<30} {:<12} {:<8} {:<12}",
            report.label,
            mark(&report.rs_to_ref),
            mark(&report.flac_t),
            mark(&report.ref_to_rs)
        );
    }
    // 失敗があれば理由を表の下にまとめて出す
    for report in reports {
        for (check, result) in [
            ("rs->ref pcm", &report.rs_to_ref),
            ("flac -t", &report.flac_t),
            ("ref->rs pcm", &report.ref_to_rs),
        ] {
            if let Err(e) = result {
                println!("  FAIL {} / {check}: {e}", report.label);
            }
        }
    }
}

/// チェック結果の表示文字列を返す
fn mark(result: &Result<(), String>) -> &'static str {
    if result.is_ok() { "ok" } else { "FAIL" }
}

/// 圧縮率の表を出力する
fn print_compression(reports: &[CaseReport]) {
    println!();
    println!(
        "== compression (frame data bytes, metadata excluded; ratio = flac-rs / reference) =="
    );
    println!(
        "{:<30} {:>10} {:>10} {:>7} {:>10} {:>7}",
        "case", "flac-rs", "ref -5", "ratio", "ref -8", "ratio"
    );
    let mut total = (0usize, 0usize, 0usize);
    for report in reports.iter().filter(|r| r.in_compression_table) {
        let Some((rs, ref5, ref8)) = report.frame_bytes else {
            println!("{:<30} (skipped: reference encode failed)", report.label);
            continue;
        };
        total = (total.0 + rs, total.1 + ref5, total.2 + ref8);
        println!(
            "{:<30} {:>10} {:>10} {:>7.3} {:>10} {:>7.3}",
            report.label,
            rs,
            ref5,
            ratio(rs, ref5),
            ref8,
            ratio(rs, ref8)
        );
    }
    println!(
        "{:<30} {:>10} {:>10} {:>7.3} {:>10} {:>7.3}",
        "(total)",
        total.0,
        total.1,
        ratio(total.0, total.1),
        total.2,
        ratio(total.0, total.2)
    );
}

/// flac-rs と本家のサイズ比 (1 より小さければ flac-rs の方が小さい)
fn ratio(rs: usize, reference: usize) -> f64 {
    rs as f64 / reference as f64
}

/// 速度計測の結果 (CLI end-to-end の wall time)
struct SpeedReport {
    /// examples の flac_encode
    rs_encode: Duration,
    /// 本家 -5 エンコード
    ref5_encode: Duration,
    /// 本家 -8 エンコード
    ref8_encode: Duration,
    /// examples の flac_decode (入力は本家 -5 の出力)
    rs_decode: Duration,
    /// 本家デコード (入力は本家 -5 の出力)
    ref_decode: Duration,
}

/// 速度を計測する
///
/// examples の flac_encode / flac_decode と本家コマンドをプロセス起動込みの
/// 同条件で計る。デコードは両者に同じ入力 (本家 -5 の出力) を与える。
fn run_speed(
    reference: &ReferenceFlac,
    work_dir: &Path,
    seconds: u32,
) -> Result<SpeedReport, String> {
    // examples のバイナリは自分と同じディレクトリ (target/<profile>) にある
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .ok_or_else(|| "failed to locate the current executable".to_string())?;
    let encode_bin = exe_dir.join("flac_encode");
    let decode_bin = exe_dir.join("flac_decode");
    if !encode_bin.is_file() || !decode_bin.is_file() {
        return Err(
            "flac_encode / flac_decode binaries are not built; run via make compare \
             (or cargo build --release -p flac_encode -p flac_decode first)"
                .to_string(),
        );
    }

    // 計測用信号 (mixed) を WAV に書き出す
    let samples = signal::mixed(44_100 * seconds as usize);
    let src_wav = work_dir.join("speed.wav");
    wav::write(&src_wav, 2, 44_100, 16, &samples)
        .map_err(|e| format!("failed to write the speed WAV: {e}"))?;

    let rs_flac = work_dir.join("speed_rs.flac");
    let rs_encode = measure(|| {
        let mut command = Command::new(&encode_bin);
        command.arg(&src_wav).arg(&rs_flac);
        command
    })?;
    let ref5_flac = work_dir.join("speed_ref5.flac");
    let ref5_encode = measure(|| {
        let mut command = reference.command();
        command.args([OsStr::new("-5"), OsStr::new("-f"), OsStr::new("-o")]);
        command.arg(&ref5_flac).arg(&src_wav);
        command
    })?;
    let ref8_flac = work_dir.join("speed_ref8.flac");
    let ref8_encode = measure(|| {
        let mut command = reference.command();
        command.args([OsStr::new("-8"), OsStr::new("-f"), OsStr::new("-o")]);
        command.arg(&ref8_flac).arg(&src_wav);
        command
    })?;

    // デコードは同じ入力 (本家 -5 の出力) を両者に与える
    let rs_out_wav = work_dir.join("speed_rs_decoded.wav");
    let rs_decode = measure(|| {
        let mut command = Command::new(&decode_bin);
        command.arg(&ref5_flac).arg(&rs_out_wav);
        command
    })?;
    let ref_out_wav = work_dir.join("speed_ref_decoded.wav");
    let ref_decode = measure(|| {
        let mut command = reference.command();
        command.args([OsStr::new("-d"), OsStr::new("-f"), OsStr::new("-o")]);
        command.arg(&ref_out_wav).arg(&ref5_flac);
        command
    })?;

    Ok(SpeedReport {
        rs_encode,
        ref5_encode,
        ref8_encode,
        rs_decode,
        ref_decode,
    })
}

/// コマンドを起動して終了までの実時間を計る (SPEED_RUNS 回の最小値)
///
/// ライブラリ内部の純粋な処理時間ではなく、プロセス起動と I/O を含む
/// CLI としての体感速度を比較する。
fn measure(build: impl Fn() -> Command) -> Result<Duration, String> {
    let mut best: Option<Duration> = None;
    for _ in 0..SPEED_RUNS {
        let mut command = build();
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let program = command.get_program().to_string_lossy().to_string();
        let start = Instant::now();
        let status = command
            .status()
            .map_err(|e| format!("failed to run {program}: {e}"))?;
        let elapsed = start.elapsed();
        if !status.success() {
            return Err(format!("{program} exited with {status}"));
        }
        best = Some(best.map_or(elapsed, |b| b.min(elapsed)));
    }
    best.ok_or_else(|| "no measurement runs".to_string())
}

/// 速度計測の結果を出力する
fn print_speed(speed: &SpeedReport, seconds: u32) {
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let times = |rs: Duration, reference: Duration| ms(rs) / ms(reference);
    println!();
    println!(
        "== speed ({seconds} s 16 bit stereo mixed signal, CLI end-to-end, best of {SPEED_RUNS}; \
         x = flac-rs time / reference time) =="
    );
    println!(
        "encode: flac-rs {:>7.1} ms | ref -5 {:>7.1} ms (x{:.2}) | ref -8 {:>7.1} ms (x{:.2})",
        ms(speed.rs_encode),
        ms(speed.ref5_encode),
        times(speed.rs_encode, speed.ref5_encode),
        ms(speed.ref8_encode),
        times(speed.rs_encode, speed.ref8_encode)
    );
    println!(
        "decode: flac-rs {:>7.1} ms | ref    {:>7.1} ms (x{:.2})   (both decode the ref -5 output)",
        ms(speed.rs_decode),
        ms(speed.ref_decode),
        times(speed.rs_decode, speed.ref_decode)
    );
}
