//! FLAC で使う CRC の実装
//!
//! - CRC-8: 多項式 x^8 + x^2 + x^1 + x^0 (0x07)、初期値 0。
//!   フレームヘッダーの保護に使う (RFC 9639 Section 9.1.8)
//! - CRC-16: 多項式 x^16 + x^15 + x^2 + x^0 (0x8005)、初期値 0。
//!   フレーム全体の保護に使う (RFC 9639 Section 9.3)
//!
//! どちらも入力を MSB-first で処理する。CRC-8 は対象が短いフレームヘッダー
//! だけなので 256 エントリのテーブルで 1 バイトずつ更新する。CRC-16 は
//! フレーム全体が対象でホットパスになるため、8 面のテーブルで 8 バイト
//! ずつ更新する (slice-by-8)。

/// CRC-8 のバイト単位更新テーブルをコンパイル時に生成する
const fn build_crc8_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u8;
        let mut j = 0;
        while j < 8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// CRC-16 の slice-by-8 用テーブルをコンパイル時に生成する
///
/// `TABLES[0]` は通常のバイト単位更新テーブル。`TABLES[k][b]` は「バイト b の
/// 後にゼロバイトが k 個続く」入力の CRC で、8 バイト窓の各バイトの寄与を
/// 独立に求めて XOR で合成できるようにする (CRC の GF(2) 線形性による)。
const fn build_crc16_tables() -> [[u16; 256]; 8] {
    let mut tables = [[0u16; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut j = 0;
        while j < 8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
            j += 1;
        }
        tables[0][i] = crc;
        i += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            // ゼロバイトを 1 つ追い足す: crc' = (crc << 8) ^ TABLES[0][crc >> 8]
            let prev = tables[k - 1][i];
            tables[k][i] = (prev << 8) ^ tables[0][(prev >> 8) as usize];
            i += 1;
        }
        k += 1;
    }
    tables
}

const CRC8_TABLE: [u8; 256] = build_crc8_table();
const CRC16_TABLES: [[u16; 256]; 8] = build_crc16_tables();

/// CRC-8 (多項式 0x07、初期値 0) のストリーミング計算器
#[derive(Debug, Clone)]
pub(crate) struct Crc8 {
    value: u8,
}

impl Crc8 {
    pub(crate) fn new() -> Self {
        Self { value: 0 }
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.value = CRC8_TABLE[usize::from(self.value ^ byte)];
        }
    }

    pub(crate) fn value(&self) -> u8 {
        self.value
    }
}

/// CRC-16 (多項式 0x8005、初期値 0) のストリーミング計算器
#[derive(Debug, Clone)]
pub(crate) struct Crc16 {
    value: u16,
}

impl Crc16 {
    pub(crate) fn new() -> Self {
        Self { value: 0 }
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.value;
        // 8 バイトずつ処理する (slice-by-8)。現在の CRC (16 bit) を先頭
        // 2 バイトへ畳み込むと残りは各入力バイト単独の寄与になり、8 面の
        // テーブル参照の XOR でまとめて 8 バイト分更新できる。バイト単位の
        // 更新はテーブル参照の直列依存が律速になるが、この形は 8 参照が
        // 互いに独立なので並列に実行される
        let (chunks, remainder) = bytes.as_chunks::<8>();
        for chunk in chunks {
            crc = CRC16_TABLES[7][usize::from((crc >> 8) as u8 ^ chunk[0])]
                ^ CRC16_TABLES[6][usize::from(crc as u8 ^ chunk[1])]
                ^ CRC16_TABLES[5][usize::from(chunk[2])]
                ^ CRC16_TABLES[4][usize::from(chunk[3])]
                ^ CRC16_TABLES[3][usize::from(chunk[4])]
                ^ CRC16_TABLES[2][usize::from(chunk[5])]
                ^ CRC16_TABLES[1][usize::from(chunk[6])]
                ^ CRC16_TABLES[0][usize::from(chunk[7])];
        }
        for &byte in remainder {
            crc = (crc << 8) ^ CRC16_TABLES[0][usize::from((crc >> 8) as u8 ^ byte)];
        }
        self.value = crc;
    }

    pub(crate) fn value(&self) -> u16 {
        self.value
    }
}

/// バイト列の CRC-8 を一括計算する
pub(crate) fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = Crc8::new();
    crc.update(bytes);
    crc.value()
}

/// バイト列の CRC-16 を一括計算する
pub(crate) fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = Crc16::new();
    crc.update(bytes);
    crc.value()
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 9639 Appendix D.1 の唯一のフレーム (0x2a-0x38):
    //   FF F8 69 18 00 00 BF 03 58 FD 03 12 8B AA 9A
    // フレームヘッダー FF F8 69 18 00 00 の CRC-8 は 0xBF (0x30 の値)、
    // CRC-16 を除くフレーム全体の CRC-16 は末尾 2 バイトの 0xAA9A
    #[test]
    fn crc8_rfc9639_appendix_d1() {
        assert_eq!(crc8(&[0xFF, 0xF8, 0x69, 0x18, 0x00, 0x00]), 0xBF);
    }

    #[test]
    fn crc16_rfc9639_appendix_d1() {
        let frame = [
            0xFF, 0xF8, 0x69, 0x18, 0x00, 0x00, 0xBF, 0x03, 0x58, 0xFD, 0x03, 0x12, 0x8B,
        ];
        assert_eq!(crc16(&frame), 0xAA9A);
    }

    #[test]
    fn crc8_empty_is_zero() {
        // 初期値 0 なので空入力の CRC は 0
        assert_eq!(crc8(&[]), 0);
    }

    #[test]
    fn crc16_empty_is_zero() {
        assert_eq!(crc16(&[]), 0);
    }

    #[test]
    fn streaming_matches_oneshot() {
        // 分割して update しても一括計算と同じ結果になる
        let data: alloc::vec::Vec<u8> = (0u16..256).map(|i| i as u8).collect();
        let mut c8 = Crc8::new();
        let mut c16 = Crc16::new();
        for chunk in data.chunks(7) {
            c8.update(chunk);
            c16.update(chunk);
        }
        assert_eq!(c8.value(), crc8(&data));
        assert_eq!(c16.value(), crc16(&data));
    }

    /// slice-by-8 の実装がバイト単位の定義どおりの計算と一致する
    ///
    /// 8 バイト窓の途中・境界・端数を網羅するため、長さ 0 から 64 の
    /// 全プレフィックスで照合する。
    #[test]
    fn slice_by_8_matches_bytewise_computation() {
        let data: alloc::vec::Vec<u8> = (0u32..64)
            .map(|i| (i.wrapping_mul(37) ^ (i << 3)) as u8)
            .collect();
        for len in 0..=data.len() {
            // バイト単位のリファレンス計算 (テーブル 0 面のみ使用)
            let mut reference: u16 = 0;
            for &byte in &data[..len] {
                reference =
                    (reference << 8) ^ CRC16_TABLES[0][usize::from((reference >> 8) as u8 ^ byte)];
            }
            assert_eq!(crc16(&data[..len]), reference, "長さ {} で不一致", len);
        }
    }

    /// テーブル駆動の実装が定義どおりのビット毎計算と全バイト値で一致する
    #[test]
    fn table_matches_bitwise_computation() {
        for i in 0u16..256 {
            let byte = i as u8;

            let mut crc8_bitwise: u8 = byte;
            for _ in 0..8 {
                crc8_bitwise = if crc8_bitwise & 0x80 != 0 {
                    (crc8_bitwise << 1) ^ 0x07
                } else {
                    crc8_bitwise << 1
                };
            }
            assert_eq!(crc8(&[byte]), crc8_bitwise, "CRC-8 バイト {:#04x}", byte);

            let mut crc16_bitwise: u16 = u16::from(byte) << 8;
            for _ in 0..8 {
                crc16_bitwise = if crc16_bitwise & 0x8000 != 0 {
                    (crc16_bitwise << 1) ^ 0x8005
                } else {
                    crc16_bitwise << 1
                };
            }
            assert_eq!(crc16(&[byte]), crc16_bitwise, "CRC-16 バイト {:#04x}", byte);
        }
    }
}
