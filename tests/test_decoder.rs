//! decoder モジュールの単体テスト
//!
//! RFC 9639 Appendix D の実例ファイルとの一致、および意図的なエラーパスを
//! 検証する。ラウンドトリップ性は PBT (pbt/tests/prop_encoder.rs 等) が担う。

use shiguredo_flac::decoder::{StreamDecoder, decode};
use shiguredo_flac::error::DecodeError;
use shiguredo_flac::metadata::MetadataBlock;

/// RFC 9639 Appendix D.1 のファイル全体 (2 チャンネル 1 サンプル)
fn rfc9639_appendix_d1_file() -> Vec<u8> {
    vec![
        0x66, 0x4c, 0x61, 0x43, 0x80, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x0f,
        0x00, 0x00, 0x0f, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x01, 0x3e, 0x84, 0xb4, 0x18,
        0x07, 0xdc, 0x69, 0x03, 0x07, 0x58, 0x6a, 0x3d, 0xad, 0x1a, 0x2e, 0x0f, 0xff, 0xf8, 0x69,
        0x18, 0x00, 0x00, 0xbf, 0x03, 0x58, 0xfd, 0x03, 0x12, 0x8b, 0xaa, 0x9a,
    ]
}

/// RFC 9639 Appendix D.2 のファイル全体
/// (side-right ステレオ + 固定予測 + SEEKTABLE + VORBIS_COMMENT + PADDING)
fn rfc9639_appendix_d2_file() -> Vec<u8> {
    vec![
        0x66, 0x4c, 0x61, 0x43, 0x00, 0x00, 0x00, 0x22, 0x00, 0x10, 0x00, 0x10, 0x00, 0x00, 0x17,
        0x00, 0x00, 0x44, 0x0a, 0xc4, 0x42, 0xf0, 0x00, 0x00, 0x00, 0x13, 0xd5, 0xb0, 0x56, 0x49,
        0x75, 0xe9, 0x8b, 0x8d, 0x8b, 0x93, 0x04, 0x22, 0x75, 0x7b, 0x81, 0x03, 0x03, 0x00, 0x00,
        0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x10, 0x04, 0x00, 0x00, 0x3a, 0x20, 0x00, 0x00, 0x00, 0x72, 0x65, 0x66,
        0x65, 0x72, 0x65, 0x6e, 0x63, 0x65, 0x20, 0x6c, 0x69, 0x62, 0x46, 0x4c, 0x41, 0x43, 0x20,
        0x31, 0x2e, 0x33, 0x2e, 0x33, 0x20, 0x32, 0x30, 0x31, 0x39, 0x30, 0x38, 0x30, 0x34, 0x01,
        0x00, 0x00, 0x00, 0x0e, 0x00, 0x00, 0x00, 0x54, 0x49, 0x54, 0x4c, 0x45, 0x3d, 0xd7, 0xa9,
        0xd7, 0x9c, 0xd7, 0x95, 0xd7, 0x9d, 0x81, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xf8, 0x69, 0x98, 0x00, 0x0f, 0x99, 0x12, 0x08, 0x67, 0x01, 0x62, 0x3d, 0x14,
        0x42, 0x99, 0x8f, 0x5d, 0xf7, 0x0d, 0x6f, 0xe0, 0x0c, 0x17, 0xca, 0xeb, 0x21, 0x00, 0x0e,
        0xe7, 0xa7, 0x7a, 0x24, 0xa1, 0x59, 0x0c, 0x12, 0x17, 0xb6, 0x03, 0x09, 0x7b, 0x78, 0x4f,
        0xaa, 0x9a, 0x33, 0xd2, 0x85, 0xe0, 0x70, 0xad, 0x5b, 0x1b, 0x48, 0x51, 0xb4, 0x01, 0x0d,
        0x99, 0xd2, 0xcd, 0x1a, 0x68, 0xf1, 0xe6, 0xb8, 0x10, 0xff, 0xf8, 0x69, 0x18, 0x01, 0x02,
        0xa4, 0x02, 0xc3, 0x82, 0xc4, 0x0b, 0xc1, 0x4a, 0x03, 0xee, 0x48, 0xdd, 0x03, 0xb6, 0x7c,
        0x13, 0x30,
    ]
}

/// RFC 9639 Appendix D.3 のファイル全体 (LPC + エスケープパーティション)
fn rfc9639_appendix_d3_file() -> Vec<u8> {
    vec![
        0x66, 0x4c, 0x61, 0x43, 0x80, 0x00, 0x00, 0x22, 0x10, 0x00, 0x10, 0x00, 0x00, 0x00, 0x1f,
        0x00, 0x00, 0x1f, 0x07, 0xd0, 0x00, 0x70, 0x00, 0x00, 0x00, 0x18, 0xf8, 0xf9, 0xe3, 0x96,
        0xf5, 0xcb, 0xcf, 0xc6, 0xdc, 0x80, 0x7f, 0x99, 0x77, 0x90, 0x6b, 0x32, 0xff, 0xf8, 0x68,
        0x02, 0x00, 0x17, 0xe9, 0x44, 0x00, 0x4f, 0x6f, 0x31, 0x3d, 0x10, 0x47, 0xd2, 0x27, 0xcb,
        0x6d, 0x09, 0x08, 0x31, 0x45, 0x2b, 0xdc, 0x28, 0x22, 0x22, 0x80, 0x57, 0xa3,
    ]
}

/// RFC 9639 Appendix D.1: 2 チャンネル 1 サンプル (verbatim + wasted bits)
#[test]
fn decode_rfc9639_appendix_d1() {
    let decoded = decode(&rfc9639_appendix_d1_file())
        .expect("RFC 9639 Appendix D.1 のデコードに成功するはず");
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.sample_rate, 44100);
    assert_eq!(decoded.bits_per_sample, 16);
    // 第 1 チャンネル 25588、第 2 チャンネル 10416 (RFC 9639 Appendix D.1.4)
    assert_eq!(decoded.samples, [25588, 10416]);
}

/// RFC 9639 Appendix D.2: side-right ステレオと固定予測
#[test]
fn decode_rfc9639_appendix_d2() {
    let decoded = decode(&rfc9639_appendix_d2_file())
        .expect("RFC 9639 Appendix D.2 のデコードに成功するはず");
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.stream_info.total_samples, 19);
    // 最初の 2 インターチャンネルサンプル (RFC 9639 Appendix D.2.7 Table 41)
    assert_eq!(&decoded.samples[..4], &[10372, 6070, 18041, 10545]);
    // 全 19 サンプルの MD5 検証まで通っている
    assert_eq!(decoded.samples.len(), 19 * 2);
    // メタデータ: STREAMINFO + SEEKTABLE + VORBIS_COMMENT + PADDING
    assert_eq!(decoded.metadata.len(), 4);
    let MetadataBlock::VorbisComment(comment) = &decoded.metadata[2] else {
        panic!("3 番目のメタデータブロックは VORBIS_COMMENT のはず");
    };
    assert_eq!(comment.vendor, "reference libFLAC 1.3.3 20190804");
    assert_eq!(comment.fields[0].name, "TITLE");
    assert_eq!(comment.fields[0].value, "שלום");
}

/// RFC 9639 Appendix D.3: LPC とエスケープパーティション
#[test]
fn decode_rfc9639_appendix_d3() {
    let decoded = decode(&rfc9639_appendix_d3_file())
        .expect("RFC 9639 Appendix D.3 のデコードに成功するはず");
    assert_eq!(decoded.channels, 1);
    assert_eq!(decoded.sample_rate, 32000);
    assert_eq!(decoded.bits_per_sample, 8);
    // 最初の 12 サンプル (RFC 9639 Appendix D.3.4 Table 49)
    assert_eq!(
        &decoded.samples[..12],
        &[0, 79, 111, 78, 8, -61, -90, -68, -13, 42, 67, 53]
    );
    assert_eq!(decoded.samples.len(), 24);
}

/// バイトを 1 つずつ feed してもデコードできる (Sans I/O の再開性)
#[test]
fn decode_frame_with_byte_by_byte_feed() {
    let file = rfc9639_appendix_d2_file();
    let mut decoder = StreamDecoder::new();
    let mut frames = Vec::new();
    for &byte in &file {
        decoder.feed(&[byte]);
        while let Some(frame) = decoder
            .decode_frame()
            .expect("フレームデコードに成功するはず")
        {
            frames.push(frame);
        }
    }
    decoder.finish();
    while let Some(frame) = decoder
        .decode_frame()
        .expect("フレームデコードに成功するはず")
    {
        frames.push(frame);
    }
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].header.block_size, 16);
    assert_eq!(frames[1].header.block_size, 3);
    assert_eq!(frames[0].first_sample_number, 0);
    assert_eq!(frames[1].first_sample_number, 16);
}

#[test]
fn decode_rejects_bad_stream_marker() {
    let mut file = rfc9639_appendix_d1_file();
    file[0] = b'g';
    assert!(matches!(
        decode(&file),
        Err(DecodeError::InvalidStreamMarker { .. })
    ));
}

#[test]
fn decode_rejects_corrupted_frame_crc() {
    let mut file = rfc9639_appendix_d1_file();
    // フレーム CRC-16 の直前のバイト (残差データ) を壊す
    let len = file.len();
    file[len - 3] ^= 0x01;
    assert!(matches!(
        decode(&file),
        Err(DecodeError::FrameCrcMismatch { .. })
    ));
}

#[test]
fn decode_rejects_corrupted_frame_header_crc() {
    let mut file = rfc9639_appendix_d1_file();
    // フレームヘッダーの CRC-8 (0x30 の位置) を壊す
    file[0x30] ^= 0x01;
    assert!(matches!(
        decode(&file),
        Err(DecodeError::FrameHeaderCrcMismatch { .. })
    ));
}

#[test]
fn decode_rejects_corrupted_md5() {
    let mut file = rfc9639_appendix_d1_file();
    // STREAMINFO の MD5 フィールド (0x1a-0x29) を壊す
    file[0x1a] ^= 0x01;
    assert!(matches!(
        decode(&file),
        Err(DecodeError::Md5Mismatch { .. })
    ));
}

#[test]
fn decode_rejects_truncated_stream() {
    let file = rfc9639_appendix_d1_file();
    // フレーム途中で切る
    let mut decoder = StreamDecoder::new();
    decoder.feed(&file[..file.len() - 5]);
    decoder.finish();
    assert!(matches!(
        decoder.decode_frame(),
        Err(DecodeError::TruncatedStream)
    ));
}

#[test]
fn decode_rejects_truncated_metadata() {
    let file = rfc9639_appendix_d1_file();
    // STREAMINFO の途中で切る
    let mut decoder = StreamDecoder::new();
    decoder.feed(&file[..20]);
    decoder.finish();
    assert!(matches!(
        decoder.decode_frame(),
        Err(DecodeError::TruncatedStream)
    ));
}

#[test]
fn decode_rejects_metadata_without_streaminfo() {
    // fLaC + PADDING (STREAMINFO なし)
    let mut file = b"fLaC".to_vec();
    file.extend_from_slice(&[0x81, 0x00, 0x00, 0x00]);
    assert!(matches!(decode(&file), Err(DecodeError::InvalidData(_))));
}

#[test]
fn decode_rejects_wrong_total_samples() {
    let mut file = rfc9639_appendix_d1_file();
    // STREAMINFO の total samples (1) を 2 に書き換える
    file[0x19] = 0x02;
    assert!(matches!(decode(&file), Err(DecodeError::InvalidData(_))));
}

#[test]
fn decode_needs_more_data_returns_none() {
    let file = rfc9639_appendix_d1_file();
    let mut decoder = StreamDecoder::new();
    decoder.feed(&file[..10]);
    // finish() していないのでデータ不足は None
    assert_eq!(
        decoder
            .decode_frame()
            .expect("フレームデコードに成功するはず"),
        None
    );
}

#[test]
fn stream_info_available_after_first_decode_attempt() {
    let file = rfc9639_appendix_d2_file();
    let mut decoder = StreamDecoder::new();
    decoder.feed(&file);
    decoder.finish();
    assert!(decoder.stream_info().is_none());
    let _ = decoder
        .decode_frame()
        .expect("フレームデコードに成功するはず");
    let info = decoder
        .stream_info()
        .expect("STREAMINFO がデコード済みのはず");
    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.min_block_size, 16);
}

#[test]
fn decode_empty_input_with_finish_is_truncated() {
    let mut decoder = StreamDecoder::new();
    decoder.finish();
    assert!(matches!(
        decoder.decode_frame(),
        Err(DecodeError::TruncatedStream)
    ));
}
