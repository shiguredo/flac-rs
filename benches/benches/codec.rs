//! エンコード / デコードのスループットベンチマーク
//!
//! 楽音を模した信号 (減衰正弦波 + 小ノイズ) とホワイトノイズの 2 種類で計測する。
//! 前者は LPC / 固定予測が効く経路、後者は verbatim / 高次 Rice の経路を通る。

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

use benches::impulse_signal;
use shiguredo_flac::decoder::decode;
use shiguredo_flac::encoder::{StreamEncoderConfig, encode};

/// ステレオ 16 bit の合成信号 (インターチャンネルサンプル数 = frames)
fn tonal_signal(frames: usize) -> Vec<i32> {
    let mut state = 0x2545F4914F6CDD1Du64;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut phase_l = 0.0f64;
    let mut phase_r = 0.0f64;
    for i in 0..frames {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let noise = ((state >> 48) as i32 - 32768) >> 8;
        // 折れ線正弦波近似 (三角波) にノイズを混ぜた楽音風の信号
        phase_l += 440.0 / 44100.0;
        phase_r += 554.37 / 44100.0;
        let tri = |p: f64| -> i32 {
            let x = p.fract();
            let v = if x < 0.5 {
                4.0 * x - 1.0
            } else {
                3.0 - 4.0 * x
            };
            (v * 18000.0) as i32
        };
        let envelope = 1.0 - (i as f64 / frames as f64) * 0.5;
        samples.push(((tri(phase_l) as f64 * envelope) as i32) + noise);
        samples.push(((tri(phase_r) as f64 * envelope) as i32) - noise);
    }
    samples
}

/// ステレオ 16 bit のホワイトノイズ
fn noise_signal(frames: usize) -> Vec<i32> {
    let mut state = 0x9E3779B97F4A7C15u64;
    (0..frames * 2)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 48) as i32 - 32768
        })
        .collect()
}

fn config() -> StreamEncoderConfig {
    StreamEncoderConfig {
        sample_rate: 44100,
        channels: 2,
        bits_per_sample: 16,
        // 圧縮率の比較条件を flac_compare と揃えて固定する (既定値と同一)
        block_size: 4096,
        ..StreamEncoderConfig::default()
    }
}

fn bench_codec(c: &mut Criterion) {
    // 44.1 kHz 2 秒相当
    const FRAMES: usize = 44100 * 2;
    // impulse は flac_compare と同じ 5 秒相当 (220500 フレーム)
    const IMPULSE_FRAMES: usize = 44100 * 5;
    let tonal = tonal_signal(FRAMES);
    let noise = noise_signal(FRAMES);
    let impulse = impulse_signal(IMPULSE_FRAMES);
    let tonal_flac = encode(config(), &tonal).expect("エンコードに成功するはず");
    let noise_flac = encode(config(), &noise).expect("エンコードに成功するはず");

    let mut group = c.benchmark_group("codec");
    group.sample_size(20);
    // インターチャンネルサンプル数/秒でスループットを表示する
    group.throughput(Throughput::Elements(FRAMES as u64));

    group.bench_function("encode_tonal", |b| {
        b.iter(|| encode(config(), black_box(&tonal)).expect("エンコードに成功するはず"))
    });
    group.bench_function("encode_noise", |b| {
        b.iter(|| encode(config(), black_box(&noise)).expect("エンコードに成功するはず"))
    });
    group.bench_function("decode_tonal", |b| {
        b.iter(|| decode(black_box(&tonal_flac)).expect("デコードに成功するはず"))
    });
    group.bench_function("decode_noise", |b| {
        b.iter(|| decode(black_box(&noise_flac)).expect("デコードに成功するはず"))
    });
    group.finish();

    // impulse は 5 秒相当と信号長が異なるため、スループット表示を正しくする
    // ために別グループにする (flac_compare の信号と同じ長さ)
    let mut group = c.benchmark_group("codec_impulse");
    group.sample_size(20);
    group.throughput(Throughput::Elements(IMPULSE_FRAMES as u64));
    group.bench_function("encode_impulse", |b| {
        b.iter(|| encode(config(), black_box(&impulse)).expect("エンコードに成功するはず"))
    });
    group.finish();
}

criterion_group!(benches, bench_codec);
criterion_main!(benches);
