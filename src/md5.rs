//! MD5 メッセージダイジェスト (RFC 1321) の依存なし実装
//!
//! STREAMINFO メタデータブロックに格納する「エンコード前オーディオデータの
//! MD5 チェックサム」の計算・検証に使う (RFC 9639 Section 8.2)。
//!
//! MD5 は暗号学的には既に安全ではないが、FLAC ではデータ破損検出のみに
//! 使われるため問題にならない。

use alloc::vec::Vec;

/// インターリーブ済みサンプル列を MD5 入力のバイト列へ変換して `buf` に格納する
///
/// FLAC の MD5 はエンコード前サンプルをインターリーブ順・signed little-endian・
/// バイト整列で並べたバイト列に対して計算する (RFC 9639 Section 8.2)。
/// サンプルごとの push は容量チェックが毎回走るため、先に全長へ resize して
/// バイト数別の一括ループで書き込む (自動ベクトル化しやすい形)。
pub(crate) fn samples_to_md5_bytes(samples: &[i32], bytes_per_sample: usize, buf: &mut Vec<u8>) {
    debug_assert!(
        (1..=4).contains(&bytes_per_sample),
        "サンプルは 1-4 バイト (実装バグ)"
    );
    buf.resize(samples.len() * bytes_per_sample, 0);
    match bytes_per_sample {
        1 => {
            for (dst, &sample) in buf.iter_mut().zip(samples) {
                *dst = sample as u8;
            }
        }
        2 => {
            for (dst, &sample) in buf.chunks_exact_mut(2).zip(samples) {
                dst.copy_from_slice(&(sample as u16).to_le_bytes());
            }
        }
        3 => {
            for (dst, &sample) in buf.chunks_exact_mut(3).zip(samples) {
                dst.copy_from_slice(&sample.to_le_bytes()[..3]);
            }
        }
        _ => {
            for (dst, &sample) in buf.chunks_exact_mut(4).zip(samples) {
                dst.copy_from_slice(&sample.to_le_bytes());
            }
        }
    }
}

/// 各ラウンドの左回転量 (RFC 1321 Section 3.4)
const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, // ラウンド 1
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, // ラウンド 2
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, // ラウンド 3
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, // ラウンド 4
];

/// 定数テーブル T[i] = floor(2^32 * abs(sin(i+1))) (RFC 1321 Section 3.4)
const SINE_TABLE: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// MD5 のストリーミング計算器 (RFC 1321)
#[derive(Debug, Clone)]
pub(crate) struct Md5 {
    /// ダイジェスト状態 (A, B, C, D)
    state: [u32; 4],
    /// 64 バイトブロックの端数バッファ
    buffer: Vec<u8>,
    /// これまでに処理した総バイト数
    total_len: u64,
}

impl Md5 {
    pub(crate) fn new() -> Self {
        Self {
            // RFC 1321 Section 3.3 の初期値
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            buffer: Vec::new(),
            total_len: 0,
        }
    }

    /// データを追加する
    pub(crate) fn update(&mut self, data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        let mut rest = data;
        // 端数バッファがあれば 64 バイトに満たしてから処理する
        if !self.buffer.is_empty() {
            let take = (64 - self.buffer.len()).min(rest.len());
            self.buffer.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.buffer.len() < 64 {
                return;
            }
            let block: [u8; 64] = self.buffer[..]
                .try_into()
                .expect("64 バイトのスライスは必ず変換できる (実装バグ)");
            self.process_block(&block);
            self.buffer.clear();
        }
        // 64 バイトブロックはバッファを経由せず直接処理する
        let mut chunks = rest.chunks_exact(64);
        for chunk in &mut chunks {
            let block: [u8; 64] = chunk
                .try_into()
                .expect("64 バイトのスライスは必ず変換できる (実装バグ)");
            self.process_block(&block);
        }
        // 端数を保存する
        self.buffer.extend_from_slice(chunks.remainder());
    }

    /// パディングを施してダイジェストを返す
    pub(crate) fn finalize(mut self) -> [u8; 16] {
        // RFC 1321 Section 3.1-3.2: 0x80 を追加し、長さ (bit) を
        // little-endian 64 bit で置ける位置 (mod 64 = 56) まで 0 埋めする
        let bit_len = self.total_len.wrapping_mul(8);
        self.update(&[0x80]);
        // update() が 64 バイトごとにブロックを処理するため端数は常に 0-63 バイト。
        // 長さフィールド 8 バイトを置ける 56 バイトちょうどまで 0 埋めする
        while self.buffer.len() != 56 {
            self.update(&[0x00]);
        }
        self.update(&bit_len.to_le_bytes());
        debug_assert!(self.buffer.is_empty(), "パディング後は端数なし (実装バグ)");

        let mut digest = [0u8; 16];
        for (i, word) in self.state.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        digest
    }

    /// 64 バイトブロックを処理する (RFC 1321 Section 3.4)
    fn process_block(&mut self, block: &[u8; 64]) {
        // ブロックを 16 個の little-endian 32 bit ワードに分解する
        let mut words = [0u32; 16];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            words[i] = u32::from_le_bytes(chunk.try_into().expect("4 バイト (実装バグ)"));
        }

        let [mut a, mut b, mut c, mut d] = self.state;

        // 64 ステップを 1 つのループで回すとステップごとに補助関数の選択
        // 分岐が入るため、RFC 1321 Section 3.4 のラウンド構造どおり
        // 4 つの分岐なしループに分ける。各ステップは
        // b = b + ((a + 補助関数 + words[g] + T[i]) <<< s) で状態を巡回する
        // ラウンド 1: F(b, c, d) = (b & c) | (!b & d)、ワード順は i
        for i in 0..16 {
            let f = (b & c) | (!b & d);
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(SINE_TABLE[i])
                .wrapping_add(words[i])
                .rotate_left(SHIFTS[i]);
            (a, b, c, d) = (d, b.wrapping_add(rotated), b, c);
        }
        // ラウンド 2: G(b, c, d) = (d & b) | (!d & c)、ワード順は (5i + 1) mod 16
        for i in 16..32 {
            let f = (d & b) | (!d & c);
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(SINE_TABLE[i])
                .wrapping_add(words[(5 * i + 1) % 16])
                .rotate_left(SHIFTS[i]);
            (a, b, c, d) = (d, b.wrapping_add(rotated), b, c);
        }
        // ラウンド 3: H(b, c, d) = b ^ c ^ d、ワード順は (3i + 5) mod 16
        for i in 32..48 {
            let f = b ^ c ^ d;
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(SINE_TABLE[i])
                .wrapping_add(words[(3 * i + 5) % 16])
                .rotate_left(SHIFTS[i]);
            (a, b, c, d) = (d, b.wrapping_add(rotated), b, c);
        }
        // ラウンド 4: I(b, c, d) = c ^ (b | !d)、ワード順は 7i mod 16
        for i in 48..64 {
            let f = c ^ (b | !d);
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(SINE_TABLE[i])
                .wrapping_add(words[(7 * i) % 16])
                .rotate_left(SHIFTS[i]);
            (a, b, c, d) = (d, b.wrapping_add(rotated), b, c);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md5(data: &[u8]) -> [u8; 16] {
        let mut ctx = Md5::new();
        ctx.update(data);
        ctx.finalize()
    }

    /// RFC 1321 Appendix A.5 のテストスイート
    #[test]
    fn rfc1321_test_suite() {
        assert_eq!(
            md5(b""),
            [
                0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8,
                0x42, 0x7e
            ]
        );
        assert_eq!(
            md5(b"a"),
            [
                0x0c, 0xc1, 0x75, 0xb9, 0xc0, 0xf1, 0xb6, 0xa8, 0x31, 0xc3, 0x99, 0xe2, 0x69, 0x77,
                0x26, 0x61
            ]
        );
        assert_eq!(
            md5(b"abc"),
            [
                0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
                0x7f, 0x72
            ]
        );
        assert_eq!(
            md5(b"message digest"),
            [
                0xf9, 0x6b, 0x69, 0x7d, 0x7c, 0xb7, 0x93, 0x8d, 0x52, 0x5a, 0x2f, 0x31, 0xaa, 0xf1,
                0x61, 0xd0
            ]
        );
        assert_eq!(
            md5(b"abcdefghijklmnopqrstuvwxyz"),
            [
                0xc3, 0xfc, 0xd3, 0xd7, 0x61, 0x92, 0xe4, 0x00, 0x7d, 0xfb, 0x49, 0x6c, 0xca, 0x67,
                0xe1, 0x3b
            ]
        );
        assert_eq!(
            md5(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"),
            [
                0xd1, 0x74, 0xab, 0x98, 0xd2, 0x77, 0xd9, 0xf5, 0xa5, 0x61, 0x1c, 0x2c, 0x9f, 0x41,
                0x9d, 0x9f
            ]
        );
        assert_eq!(
            md5(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            [
                0x57, 0xed, 0xf4, 0xa2, 0x2b, 0xe3, 0xc9, 0x55, 0xac, 0x49, 0xda, 0x2e, 0x21, 0x07,
                0xb6, 0x7a
            ]
        );
    }

    /// RFC 9639 Appendix D.1: サンプル 25588, 10416 (16 bit) を little-endian で
    /// 並べた 0xf4 0x63 0xb0 0x28 の MD5 が STREAMINFO の値と一致する
    #[test]
    fn rfc9639_appendix_d1_audio_md5() {
        assert_eq!(
            md5(&[0xf4, 0x63, 0xb0, 0x28]),
            [
                0x3e, 0x84, 0xb4, 0x18, 0x07, 0xdc, 0x69, 0x03, 0x07, 0x58, 0x6a, 0x3d, 0xad, 0x1a,
                0x2e, 0x0f
            ]
        );
    }

    /// サンプル → バイト列変換がサンプルごとの little-endian 切り出しと一致する
    #[test]
    fn samples_to_md5_bytes_matches_per_sample_conversion() {
        // 負値・境界値を含むサンプル列
        let samples: [i32; 7] = [0, 1, -1, 127, -128, 0x12345678, -0x12345678];
        for bytes_per_sample in 1..=4usize {
            let mut expected = Vec::new();
            for &sample in &samples {
                expected.extend_from_slice(&sample.to_le_bytes()[..bytes_per_sample]);
            }
            let mut actual = Vec::new();
            samples_to_md5_bytes(&samples, bytes_per_sample, &mut actual);
            assert_eq!(actual, expected, "{} バイトで不一致", bytes_per_sample);
        }
    }

    /// 変換バッファの再利用で前回の内容が残らない (縮む方向も正しい)
    #[test]
    fn samples_to_md5_bytes_reuses_buffer() {
        let mut buf = Vec::new();
        samples_to_md5_bytes(&[0x0102, 0x0304, 0x0506], 2, &mut buf);
        assert_eq!(buf, [0x02, 0x01, 0x04, 0x03, 0x06, 0x05]);
        // 短い列で呼び直すと長さも内容も新しい列のものになる
        samples_to_md5_bytes(&[-2], 2, &mut buf);
        assert_eq!(buf, [0xFE, 0xFF]);
    }

    #[test]
    fn streaming_matches_oneshot() {
        // 分割して update しても一括計算と同じ結果になる
        let data: Vec<u8> = (0u32..1000).map(|i| (i % 251) as u8).collect();
        let mut ctx = Md5::new();
        for chunk in data.chunks(17) {
            ctx.update(chunk);
        }
        assert_eq!(ctx.finalize(), md5(&data));
    }

    #[test]
    fn padding_boundary_lengths() {
        // パディング境界 (55, 56, 57, 63, 64, 65 バイト) で正しく動くこと
        // (別実装との照合は rfc1321_test_suite で済んでいるので、ここでは
        // ストリーミングとの一致のみ確認する)
        for len in [55usize, 56, 57, 63, 64, 65, 119, 120, 128] {
            let data: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let mut ctx = Md5::new();
            for &b in &data {
                ctx.update(&[b]);
            }
            assert_eq!(ctx.finalize(), md5(&data), "長さ {} で不一致", len);
        }
    }
}
