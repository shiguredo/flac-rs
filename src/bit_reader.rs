//! MSB-first のビットリーダー
//!
//! FLAC のビットストリームは全て big-endian (MSB-first) で格納される
//! (RFC 9639 Section 5)。本モジュールはバイト列スライス上にビット単位の
//! 読み取りカーソルを提供する。
//!
//! 未読ビットは 64 bit のキャッシュに MSB 側から詰めて保持し、読み取りの
//! 主経路をシフト演算だけにする (ビットライターのアキュムレータと対称の
//! 構成)。キャッシュへの補充はバイト単位で行う。
//!
//! データ不足は `BitReadError::UnexpectedEof` で表現する。呼び出し側
//! (デコーダー) は入力終端に達しているかどうかに応じて「追加データ待ち」か
//! 「壊れたストリーム」かを判断する。`UnexpectedEof` を返したリーダーの
//! 読み取り位置は不定 (単進符号などは途中まで読み進む) のため、呼び出し側は
//! リーダーを作り直すこと。

/// ビット読み取りのエラー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitReadError {
    /// スライスの終端に達した (データ不足)
    UnexpectedEof,
}

/// MSB-first のビットリーダー
///
/// 読み取り位置は「キャッシュへ取り込んだバイト数 - キャッシュ内の未読
/// ビット数」で保持する。`position_bits()` で現在位置を保存し、失敗時に
/// 呼び出し側でリーダーを作り直すことで巻き戻しを実現する。
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    /// 未読ビットを MSB 側から詰めたキャッシュ (下位は 0)
    cache: u64,
    /// `cache` に入っている未読ビット数 (0-64)
    cache_bits: u32,
    /// `data` 内で次にキャッシュへ取り込むバイト位置
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            cache: 0,
            cache_bits: 0,
            pos: 0,
        }
    }

    /// 現在の読み取り位置 (ビット単位)
    pub(crate) fn position_bits(&self) -> usize {
        self.pos * 8 - self.cache_bits as usize
    }

    /// 読み取り位置がバイト境界にあるか
    pub(crate) fn is_byte_aligned(&self) -> bool {
        self.cache_bits.is_multiple_of(8)
    }

    /// キャッシュへ未読データをバイト単位で補充する
    ///
    /// 取り込みは消費ではないため、読み取り位置は変わらない。
    #[inline(always)]
    fn refill(&mut self) {
        // 高速パス: 8 バイトの窓を 1 回読み、空きに収まるバイト数だけ足す
        if let Some(window) = self.data.get(self.pos..self.pos + 8) {
            let word = u64::from_be_bytes(window.try_into().expect("8 バイト固定 (実装バグ)"));
            // 空きビット数をバイト単位に切り下げる
            let take_bits = (64 - self.cache_bits) & !7;
            if take_bits == 0 {
                return;
            }
            // word の上位 take_bits ビットをキャッシュの未読末尾へ詰める。
            // どちらのシフト量も 0-56 に収まる
            self.cache |= (word >> (64 - take_bits)) << (64 - self.cache_bits - take_bits);
            self.pos += (take_bits / 8) as usize;
            self.cache_bits += take_bits;
            return;
        }
        // 低速パス (スライス末尾付近): 1 バイトずつ足す
        while self.cache_bits <= 56 && self.pos < self.data.len() {
            self.cache |= u64::from(self.data[self.pos]) << (56 - self.cache_bits);
            self.pos += 1;
            self.cache_bits += 8;
        }
    }

    /// 1 ビット読む
    #[inline]
    pub(crate) fn read_bit(&mut self) -> Result<bool, BitReadError> {
        Ok(self.read_u64(1)? == 1)
    }

    /// `bits` ビット (1-57) をキャッシュから読む (再帰しない内部ヘルパー)
    ///
    /// 再帰があるとインライン展開が抑止されるため、`read_u64` の分割読みと
    /// 通常読みの両方からこのヘルパーを呼ぶ形にする。
    #[inline(always)]
    fn read_short(&mut self, bits: u32) -> Result<u64, BitReadError> {
        debug_assert!((1..=57).contains(&bits), "read_short は 1-57 ビットまで");
        if self.cache_bits < bits {
            self.refill();
            if self.cache_bits < bits {
                return Err(BitReadError::UnexpectedEof);
            }
        }
        // bits は 1-57 なのでシフト量は溢れない
        let value = self.cache >> (64 - bits);
        self.cache <<= bits;
        self.cache_bits -= bits;
        Ok(value)
    }

    /// `bits` ビット (0-64) を符号なし整数として読む
    #[inline(always)]
    pub(crate) fn read_u64(&mut self, bits: u32) -> Result<u64, BitReadError> {
        debug_assert!(bits <= 64, "read_u64 は最大 64 ビットまで");
        if bits == 0 {
            return Ok(0);
        }
        // キャッシュはバイト単位補充のため、読み取り位置の端数によっては
        // 一度に 57 bit までしか満たせない。それを超える読みは 2 回に分ける
        if bits > 57 {
            let hi = self.read_short(bits - 32)?;
            let lo = self.read_short(32)?;
            return Ok((hi << 32) | lo);
        }
        self.read_short(bits)
    }

    /// `bits` ビット (0-32) を符号なし整数として読む
    #[inline]
    pub(crate) fn read_u32(&mut self, bits: u32) -> Result<u32, BitReadError> {
        debug_assert!(bits <= 32, "read_u32 は最大 32 ビットまで");
        Ok(self.read_u64(bits)? as u32)
    }

    /// `bits` ビット (1-64) を signed two's complement として読み、符号拡張して返す
    #[inline]
    pub(crate) fn read_i64(&mut self, bits: u32) -> Result<i64, BitReadError> {
        debug_assert!((1..=64).contains(&bits), "read_i64 は 1-64 ビットまで");
        let raw = self.read_u64(bits)?;
        // 符号ビットが立っていれば上位を 1 で埋める
        if bits < 64 && (raw >> (bits - 1)) & 1 == 1 {
            Ok((raw | (u64::MAX << bits)) as i64)
        } else {
            Ok(raw as i64)
        }
    }

    /// 単進符号 (unary) を読む: 0 ビットの個数を数え、1 ビットで終端する
    /// (RFC 9639 Section 5)
    #[inline]
    pub(crate) fn read_unary(&mut self) -> Result<u64, BitReadError> {
        let mut count: u64 = 0;
        loop {
            if self.cache == 0 {
                // キャッシュの下位は常に 0 のため、未読ビットは全て 0
                count += u64::from(self.cache_bits);
                self.cache_bits = 0;
                self.refill();
                if self.cache_bits == 0 {
                    // 補充できなければ終端記号の 1 が現れないまま EOF
                    return Err(BitReadError::UnexpectedEof);
                }
                continue;
            }
            // キャッシュが 0 でなければ最初の 1 は必ず未読ビットの範囲内にある
            let zeros = self.cache.leading_zeros();
            debug_assert!(zeros < self.cache_bits, "キャッシュの下位は 0 (実装バグ)");
            count += u64::from(zeros);
            // zeros + 1 == 64 のケースがあるため 2 段でシフトする
            self.cache = (self.cache << zeros) << 1;
            self.cache_bits -= zeros + 1;
            return Ok(count);
        }
    }

    /// Rice 符号 1 個を読む (RFC 9639 Section 9.2.7.2)
    ///
    /// 単進符号の quotient に続いて `parameter` ビットの remainder を読み、
    /// folded 値 `(quotient << parameter) | remainder` を返す。
    #[inline(always)]
    pub(crate) fn read_rice(&mut self, parameter: u32) -> Result<u64, BitReadError> {
        debug_assert!(
            parameter <= 30,
            "Rice パラメータは 0-30 (RFC 9639 Section 9.2.7)"
        );
        // 高速パス: 終端の 1 と remainder がキャッシュ内で完結する場合、
        // 1 回のキャッシュ参照で quotient と remainder をまとめて取り出す
        if self.cache != 0 {
            let zeros = self.cache.leading_zeros();
            let total = zeros + 1 + parameter;
            if total <= self.cache_bits {
                // 終端の 1 を捨てて先頭に寄せると、上位 parameter ビットが
                // remainder になる。シフト量は全て 0-63 に収まる
                let shifted = (self.cache << zeros) << 1;
                let remainder = (shifted >> 1) >> (63 - parameter);
                self.cache = shifted << parameter;
                self.cache_bits -= total;
                return Ok((u64::from(zeros) << parameter) | remainder);
            }
        }
        // 低速パス (キャッシュの補充をまたぐ場合): 2 段で読む
        let quotient = self.read_unary()?;
        let remainder = self.read_u64(parameter)?;
        Ok((quotient << parameter) | remainder)
    }

    /// バイト境界から `len` バイトを読む
    ///
    /// 呼び出し側でバイト境界に揃っていることを保証すること。
    pub(crate) fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], BitReadError> {
        debug_assert!(self.is_byte_aligned(), "read_bytes はバイト境界からのみ");
        // キャッシュに取り込み済みの未読バイトを巻き戻し、スライスから直接返す
        let start = self.pos - (self.cache_bits / 8) as usize;
        let end = start.checked_add(len).ok_or(BitReadError::UnexpectedEof)?;
        if end > self.data.len() {
            return Err(BitReadError::UnexpectedEof);
        }
        self.cache = 0;
        self.cache_bits = 0;
        self.pos = end;
        Ok(&self.data[start..end])
    }

    /// 次のバイト境界まで読み飛ばし、読み飛ばした分のビットを返す
    pub(crate) fn align_to_byte(&mut self) -> Result<u64, BitReadError> {
        // 読み取り位置の端数はキャッシュ内の未読ビット数の端数と一致する
        self.read_u64(self.cache_bits % 8)
    }

    /// 保持しているスライスの `[start, end)` バイト範囲への参照
    ///
    /// CRC 計算のために読み取り済み範囲を参照する用途で使う。
    pub(crate) fn data_range(&self, start: usize, end: usize) -> &'a [u8] {
        &self.data[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bits_msb_first() {
        // 0b1010_1100 0b0101_0011
        let data = [0xAC, 0x53];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_u32(1).unwrap(), 0b1);
        assert_eq!(reader.read_u32(3).unwrap(), 0b010);
        assert_eq!(reader.read_u32(4).unwrap(), 0b1100);
        assert_eq!(reader.read_u32(8).unwrap(), 0x53);
        // 全部読み切ったので次はデータ不足
        assert_eq!(reader.read_u32(1), Err(BitReadError::UnexpectedEof));
    }

    #[test]
    fn read_u64_across_bytes() {
        let data = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11];
        let mut reader = BitReader::new(&data);
        // バイト境界をまたぐ 4 + 64 ビット読み
        assert_eq!(reader.read_u32(4).unwrap(), 0x1);
        assert_eq!(reader.read_u64(64).unwrap(), 0x23456789ABCDEF01);
    }

    #[test]
    fn read_u64_58_bits_with_offset() {
        // 端数位置からの 58-64 ビット読みは内部で 2 回に分かれる
        let data = [0xFF; 17];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_u32(3).unwrap(), 0b111);
        assert_eq!(reader.read_u64(58).unwrap(), (1u64 << 58) - 1);
        assert_eq!(reader.read_u64(64).unwrap(), u64::MAX);
        assert_eq!(reader.position_bits(), 3 + 58 + 64);
    }

    #[test]
    fn read_zero_bits() {
        let data = [0xFF];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_u64(0).unwrap(), 0);
        assert_eq!(reader.position_bits(), 0);
    }

    #[test]
    fn read_i64_sign_extension() {
        // 4 ビットの -1 (0b1111) と 3 (0b0011)
        let data = [0b1111_0011];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_i64(4).unwrap(), -1);
        assert_eq!(reader.read_i64(4).unwrap(), 3);
    }

    #[test]
    fn read_i64_full_64bits() {
        let data = u64::MAX.to_be_bytes();
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_i64(64).unwrap(), -1);
    }

    #[test]
    fn read_unary_basic() {
        // 5 は 0b000001 (RFC 9639 Section 5)
        let data = [0b0000_0110, 0b0000_0000];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_unary().unwrap(), 5);
        // 続き: 0b10... → 0 個の 0 のあと 1
        assert_eq!(reader.read_unary().unwrap(), 0);
        // 残り 8 ビットは全部 0 なので終端記号の 1 が現れず EOF
        assert_eq!(reader.read_unary(), Err(BitReadError::UnexpectedEof));
    }

    #[test]
    fn read_unary_across_words() {
        // 9 バイト (72 ビット) の 0 の後に 1: キャッシュの補充をまたぐ
        let mut data = [0u8; 10];
        data[9] = 0b1000_0000;
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_unary().unwrap(), 72);
        assert_eq!(reader.position_bits(), 73);
    }

    #[test]
    fn read_rice_matches_two_step_read() {
        // RFC 9639 Section 9.2.7.2 の例: folded 38、パラメータ 3 は
        // quotient 4 (0b00001) + remainder 6 (0b110) = 0b00001110
        let data = [0b0000_1110];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_rice(3).unwrap(), 38);

        // キャッシュの補充をまたぐ長い quotient (低速パス) も同じ値になる
        let mut data = [0u8; 12];
        data[8] = 0b0000_0001; // 71 個の 0 + 終端 1
        data[9] = 0b1010_0000; // remainder 0b101
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_rice(3).unwrap(), (71 << 3) | 0b101);

        // パラメータ 0 (remainder なし) は unary と同じ
        let data = [0b0010_0000];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_rice(0).unwrap(), 2);

        // データ不足
        let data = [0b0000_0000];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_rice(3), Err(BitReadError::UnexpectedEof));
    }

    #[test]
    fn read_bytes_and_align() {
        let data = [0xAB, 0xCD, 0xEF];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_u32(3).unwrap(), 0b101);
        // バイト境界まで読み飛ばす (残り 5 ビット: 0b01011)
        assert_eq!(reader.align_to_byte().unwrap(), 0b01011);
        assert!(reader.is_byte_aligned());
        assert_eq!(reader.read_bytes(2).unwrap(), &[0xCD, 0xEF]);
        assert_eq!(reader.read_bytes(1), Err(BitReadError::UnexpectedEof));
    }

    #[test]
    fn read_bytes_after_cached_read() {
        // キャッシュに取り込み済みのバイトが read_bytes で正しく巻き戻る
        let data = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_u32(8).unwrap(), 0x12);
        // この時点でキャッシュには後続バイトが取り込まれている
        assert_eq!(reader.read_bytes(3).unwrap(), &[0x34, 0x56, 0x78]);
        assert_eq!(reader.position_bits(), 4 * 8);
        assert_eq!(reader.read_u32(8).unwrap(), 0x9A);
    }

    #[test]
    fn align_at_boundary_is_noop() {
        let data = [0xFF];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.align_to_byte().unwrap(), 0);
        assert_eq!(reader.position_bits(), 0);
    }
}
