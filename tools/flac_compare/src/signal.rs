//! 比較用の合成信号
//!
//! すべて固定シードの LCG による決定的な信号で、実行のたびに同じ比較結果が
//! 得られる。信号種はエンコーダーの主要な経路 (CONSTANT / 固定予測 / LPC /
//! 高エントロピーの高次 Rice / wasted bits / ステレオデコリレーション) を
//! 一通り通るように選んでいる。
//!
//! 位相の刻みは 44.1 kHz 再生を想定した値だが、生成される周波数の正確さは
//! 比較結果に影響しないため、他のサンプルレートのケースにもそのまま使う。

/// 線形合同法で疑似乱数の次状態に進める (Knuth の MMIX 定数)
fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

/// 16 bit 相当のホワイトノイズ 1 サンプル (-32768..=32767) を取り出す
fn lcg_noise16(state: &mut u64) -> i32 {
    (lcg_next(state) >> 48) as i32 - 32768
}

/// 全サンプルが 0 の無音 (CONSTANT サブフレームの経路)
pub fn silence(frames: usize) -> Vec<i32> {
    vec![0; frames * 2]
}

/// 減衰する三角波 + 小ノイズの楽音風ステレオ信号 (LPC の経路)
///
/// benches/benches/codec.rs の tonal_signal と同じ信号。`scale` は振幅の倍率で、
/// 16 bit なら 1、24 bit なら 256 を渡す (三角波 ±18000 × scale は 24 bit でも
/// オーバーフローしない)。
pub fn tonal(frames: usize, scale: i32) -> Vec<i32> {
    let mut state = 0x2545F4914F6CDD1Du64;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut phase_l = 0.0f64;
    let mut phase_r = 0.0f64;
    for i in 0..frames {
        let noise = (lcg_noise16(&mut state) >> 8) * scale;
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
            (v * 18000.0) as i32 * scale
        };
        let envelope = 1.0 - (i as f64 / frames as f64) * 0.5;
        samples.push(((tri(phase_l) as f64 * envelope) as i32) + noise);
        samples.push(((tri(phase_r) as f64 * envelope) as i32) - noise);
    }
    samples
}

/// tonal の L チャンネルだけを取り出したモノラル信号
pub fn tonal_mono(frames: usize) -> Vec<i32> {
    tonal(frames, 1).into_iter().step_by(2).collect()
}

/// 矩形波 (固定予測の経路。本家 -8 との圧縮率差が最も出る信号)
///
/// 周期がサンプル数の整数倍に乗らないよう、周波数は 440.25 / 554.37 Hz とする。
pub fn square(frames: usize) -> Vec<i32> {
    let mut samples = Vec::with_capacity(frames * 2);
    let mut phase_l = 0.0f64;
    let mut phase_r = 0.0f64;
    for _ in 0..frames {
        phase_l = (phase_l + 440.25 / 44100.0).fract();
        phase_r = (phase_r + 554.37 / 44100.0).fract();
        samples.push(if phase_l < 0.5 { 20000 } else { -20000 });
        samples.push(if phase_r < 0.5 { 18000 } else { -18000 });
    }
    samples
}

/// 100 Hz から 8 kHz への正弦波スイープ (非定常信号、次数選択の経路)
pub fn sweep(frames: usize) -> Vec<i32> {
    let mut samples = Vec::with_capacity(frames * 2);
    let mut phase = 0.0f64;
    for i in 0..frames {
        let t = i as f64 / frames as f64;
        let freq = 100.0 + (8000.0 - 100.0) * t;
        phase += freq / 44100.0;
        let v = (phase * core::f64::consts::TAU).sin();
        // R は振幅を変えてチャンネル間に差を作る
        samples.push((v * 16000.0) as i32);
        samples.push((v * 12000.0) as i32);
    }
    samples
}

/// フルスケールのステレオホワイトノイズ (圧縮がほぼ効かない高次 Rice の経路)
///
/// benches/benches/codec.rs の noise_signal と同じ信号。
pub fn noise(frames: usize) -> Vec<i32> {
    let mut state = 0x9E3779B97F4A7C15u64;
    (0..frames * 2).map(|_| lcg_noise16(&mut state)).collect()
}

/// 小振幅ノイズ (低ビットレンジの Rice の経路)
pub fn quiet(frames: usize) -> Vec<i32> {
    let mut state = 0x0123456789ABCDEFu64;
    (0..frames * 2)
        .map(|_| lcg_noise16(&mut state) >> 9)
        .collect()
}

/// 疎なインパルス列 (ほぼ無音に時折スパイク、固定予測と低次 Rice の経路)
pub fn impulse(frames: usize) -> Vec<i32> {
    let mut state = 0xDEADBEEFCAFEF00Du64;
    let mut samples = vec![0i32; frames * 2];
    let mut pos = 0usize;
    loop {
        // 次のスパイクまで 300-811 サンプル進める
        pos += 300 + (lcg_next(&mut state) >> 55) as usize;
        if pos >= frames {
            break;
        }
        // L / R 逆相のスパイク (ステレオデコリレーションの side が立つ形)
        let value = lcg_noise16(&mut state);
        samples[pos * 2] = value;
        samples[pos * 2 + 1] = -value;
    }
    samples
}

/// 下位 4 bit が常に 0 の信号 (wasted bits の経路)
pub fn wasted(frames: usize) -> Vec<i32> {
    tonal(frames, 1).into_iter().map(|s| s & !0xF).collect()
}

/// 三角波と強めのノイズの混合 (LPC と Rice の両方に負荷がかかる中間的な信号)
///
/// 速度計測用の信号もこれで生成する。
pub fn mixed(frames: usize) -> Vec<i32> {
    let mut state = 0xF0E1D2C3B4A59687u64;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut phase_l = 0.0f64;
    let mut phase_r = 0.0f64;
    let tri = |p: f64| -> f64 {
        if p < 0.5 {
            4.0 * p - 1.0
        } else {
            3.0 - 4.0 * p
        }
    };
    for _ in 0..frames {
        phase_l = (phase_l + 330.0 / 44100.0).fract();
        phase_r = (phase_r + 415.3 / 44100.0).fract();
        let noise_l = lcg_noise16(&mut state) / 8;
        let noise_r = lcg_noise16(&mut state) / 8;
        samples.push((tri(phase_l) * 12000.0) as i32 + noise_l);
        samples.push((tri(phase_r) * 12000.0) as i32 + noise_r);
    }
    samples
}
