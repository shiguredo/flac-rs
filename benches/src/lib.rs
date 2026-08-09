//! エンコード / デコードのスループットベンチマーク用の合成信号
//!
//! bench バイナリと照合テストの両方から参照するため、クレートの lib として
//! 公開する。

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

/// 疎なインパルス列 (ほぼ無音に時折スパイク、固定予測と低次 Rice の経路)
///
/// tools/flac_compare の signal::impulse と同じ固定シード・同じ生成手順の
/// 複製 (bin crate の mod signal は import できないため)。生成結果が元と
/// 一致することは tests/impulse_signal.rs の照合テストで確認する。
pub fn impulse_signal(frames: usize) -> Vec<i32> {
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
