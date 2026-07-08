//! MSB-first のビットライター
//!
//! FLAC のビットストリームは全て big-endian (MSB-first) で格納される
//! (RFC 9639 Section 5)。本モジュールは `Vec<u8>` にビット単位で書き込む
//! ライターを提供する。

use alloc::vec::Vec;

/// MSB-first のビットライター
///
/// 未出力のビットを 64 bit のアキュムレータに MSB 側から詰め、満杯に
/// なったら 8 バイトまとめて出力バッファへ押し出す (ビットリーダーの
/// 8 バイトワード読みと対称の構成)。
pub(crate) struct BitWriter {
    buf: Vec<u8>,
    /// 未出力のビットを MSB 側から詰めたアキュムレータ (下位は 0)
    acc: u64,
    /// `acc` に入っているビット数 (0-63)
    acc_bits: u32,
}

impl BitWriter {
    pub(crate) fn new() -> Self {
        Self {
            buf: Vec::new(),
            acc: 0,
            acc_bits: 0,
        }
    }

    /// 既存のバイト列の続きから書き込むライターを作る
    ///
    /// フレームを出力バッファへ直接書き足すために使う。書き込み結果は
    /// `into_bytes()` で取り出すこと。
    pub(crate) fn resume(buf: Vec<u8>) -> Self {
        Self {
            buf,
            acc: 0,
            acc_bits: 0,
        }
    }

    /// 書き込み位置がバイト境界にあるか
    pub(crate) fn is_byte_aligned(&self) -> bool {
        self.acc_bits.is_multiple_of(8)
    }

    /// 満杯 (64 bit) のアキュムレータを出力バッファへ押し出す
    fn flush_full_acc(&mut self) {
        debug_assert_eq!(self.acc_bits, 64, "flush_full_acc は満杯のときのみ");
        self.buf.extend_from_slice(&self.acc.to_be_bytes());
        self.acc = 0;
        self.acc_bits = 0;
    }

    /// アキュムレータ内の完成済みバイトを出力バッファへ押し出す
    fn flush_complete_bytes(&mut self) {
        while self.acc_bits >= 8 {
            self.buf.push((self.acc >> 56) as u8);
            self.acc <<= 8;
            self.acc_bits -= 8;
        }
    }

    /// `value` の下位 `bits` ビット (0-64) を書き込む
    pub(crate) fn write_u64(&mut self, value: u64, bits: u32) {
        debug_assert!(bits <= 64, "write_u64 は最大 64 ビットまで");
        debug_assert!(
            bits == 64 || value < (1u64 << bits),
            "value が bits に収まらない (実装バグ)"
        );
        if bits == 0 {
            return;
        }
        let space = 64 - self.acc_bits;
        if bits <= space {
            // 空きに全部入る。bits >= 1 なのでシフト量は 0-63 に収まる
            self.acc |= value << (space - bits);
            self.acc_bits += bits;
            if self.acc_bits == 64 {
                self.flush_full_acc();
            }
        } else {
            // 空きを value の上位ビットで満たして押し出し、残りの下位ビットを
            // 新しいアキュムレータへ。bits > space >= 1 なのでシフト量は
            // どちらも 1-63 に収まる
            self.acc |= value >> (bits - space);
            self.buf.extend_from_slice(&self.acc.to_be_bytes());
            let rest = bits - space;
            self.acc = value << (64 - rest);
            self.acc_bits = rest;
        }
    }

    /// `value` の下位 `bits` ビット (0-32) を書き込む
    pub(crate) fn write_u32(&mut self, value: u32, bits: u32) {
        self.write_u64(u64::from(value), bits);
    }

    /// signed two's complement として `value` を `bits` ビット (1-64) で書き込む
    ///
    /// 呼び出し側で `value` が `bits` ビットに収まることを保証すること。
    pub(crate) fn write_i64(&mut self, value: i64, bits: u32) {
        debug_assert!((1..=64).contains(&bits), "write_i64 は 1-64 ビットまで");
        debug_assert!(
            bits == 64 || (-(1i64 << (bits - 1))..(1i64 << (bits - 1))).contains(&value),
            "value が bits に収まらない (実装バグ)"
        );
        let mask = if bits == 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };
        self.write_u64((value as u64) & mask, bits);
    }

    /// 単進符号 (unary) で書き込む: `value` 個の 0 ビットのあとに 1 ビット
    /// (RFC 9639 Section 5)
    pub(crate) fn write_unary(&mut self, value: u64) {
        let mut zeros = value;
        let space = u64::from(64 - self.acc_bits);
        if zeros >= space {
            // アキュムレータの残りを 0 で満たして押し出す
            self.buf.extend_from_slice(&self.acc.to_be_bytes());
            self.acc = 0;
            self.acc_bits = 0;
            zeros -= space;
            // 丸ごとゼロのワードをまとめて追加する
            if zeros >= 64 {
                self.buf
                    .resize(self.buf.len() + (zeros / 64) as usize * 8, 0);
                zeros %= 64;
            }
        }
        // 残りの 0 (0-63 個) は詰めるビット数を進めるだけでよい。
        // このあと終端の 1 を立ててもアキュムレータに収まる
        self.acc_bits += zeros as u32;
        self.acc |= 1 << (63 - self.acc_bits);
        self.acc_bits += 1;
        if self.acc_bits == 64 {
            self.flush_full_acc();
        }
    }

    /// バイト列を書き込む
    ///
    /// バイト境界に揃っていなくても正しく動作する。
    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        if self.is_byte_aligned() {
            self.flush_complete_bytes();
            self.buf.extend_from_slice(bytes);
        } else {
            for &b in bytes {
                self.write_u64(u64::from(b), 8);
            }
        }
    }

    /// 次のバイト境界まで 0 ビットで埋める
    pub(crate) fn align_to_byte(&mut self) {
        let pad = (8 - self.acc_bits % 8) % 8;
        if pad > 0 {
            self.write_u64(0, pad);
        }
    }

    /// これまでに書き込んだ完成済みバイト数
    ///
    /// 書きかけの端数ビットは数えない。
    pub(crate) fn byte_len(&self) -> usize {
        self.buf.len() + (self.acc_bits / 8) as usize
    }

    /// バイト境界まで 0 埋めして書き込み結果を取り出す
    pub(crate) fn into_bytes(mut self) -> Vec<u8> {
        self.align_to_byte();
        self.flush_complete_bytes();
        self.buf
    }

    /// 完成済みバイト列への参照
    ///
    /// 呼び出し側でバイト境界に揃っていることを保証すること。
    pub(crate) fn as_bytes(&mut self) -> &[u8] {
        debug_assert!(self.is_byte_aligned(), "as_bytes はバイト境界でのみ");
        self.flush_complete_bytes();
        &self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_bits_msb_first() {
        let mut writer = BitWriter::new();
        writer.write_u32(0b1, 1);
        writer.write_u32(0b010, 3);
        writer.write_u32(0b1100, 4);
        writer.write_u32(0x53, 8);
        assert_eq!(writer.into_bytes(), [0xAC, 0x53]);
    }

    #[test]
    fn write_unaligned_padding() {
        let mut writer = BitWriter::new();
        writer.write_u32(0b101, 3);
        // 残り 5 ビットは 0 埋めされる
        assert_eq!(writer.into_bytes(), [0b1010_0000]);
    }

    #[test]
    fn write_i64_negative() {
        let mut writer = BitWriter::new();
        writer.write_i64(-1, 4);
        writer.write_i64(3, 4);
        assert_eq!(writer.into_bytes(), [0b1111_0011]);
    }

    #[test]
    fn write_i64_full_64bits() {
        let mut writer = BitWriter::new();
        writer.write_i64(-1, 64);
        assert_eq!(writer.into_bytes(), u64::MAX.to_be_bytes());
    }

    #[test]
    fn write_unary_basic() {
        let mut writer = BitWriter::new();
        writer.write_unary(5);
        writer.write_unary(0);
        // 0b000001 + 0b1 = 0b0000011 + 1 ビット 0 埋め
        assert_eq!(writer.into_bytes(), [0b0000_0110]);
    }

    #[test]
    fn write_bytes_after_alignment() {
        let mut writer = BitWriter::new();
        writer.write_u32(0b101, 3);
        writer.align_to_byte();
        assert!(writer.is_byte_aligned());
        writer.write_bytes(&[0xCD, 0xEF]);
        assert_eq!(writer.byte_len(), 3);
        assert_eq!(writer.into_bytes(), [0b1010_0000, 0xCD, 0xEF]);
    }

    #[test]
    fn roundtrip_with_bit_reader() {
        use crate::bit_reader::BitReader;

        let mut writer = BitWriter::new();
        writer.write_u64(0x123456789A, 40);
        writer.write_i64(-12345, 20);
        writer.write_unary(17);
        writer.write_u32(0b11, 2);
        let bytes = writer.into_bytes();

        let mut reader = BitReader::new(&bytes);
        assert_eq!(
            reader.read_u64(40).expect("40ビット読み取りに成功するはず"),
            0x123456789A
        );
        assert_eq!(
            reader
                .read_i64(20)
                .expect("符号付き20ビット読み取りに成功するはず"),
            -12345
        );
        assert_eq!(
            reader.read_unary().expect("unary読み取りに成功するはず"),
            17
        );
        assert_eq!(
            reader.read_u32(2).expect("2ビット読み取りに成功するはず"),
            0b11
        );
    }
}
