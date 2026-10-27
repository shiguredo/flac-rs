//! metadata モジュールの PBT
//!
//! 各メタデータブロックのエンコード → デコードのラウンドトリップを検証する。

use shiguredo_flac::metadata::{Application, MetadataBlock, SeekPoint, SeekTable, StreamInfo};

/// テストケース数
const CASES: usize = 128;

/// 有効な STREAMINFO を生成する
///
/// `max_block_size >= min_block_size` を構成的に保証するため、先に
/// `min_block_size` を選び、`max_block_size` をその値以上から選ぶ。
fn stream_info(ctx: &mut noprop::TestCaseContext) -> StreamInfo {
    let min_block_size = noprop::sample_u16_in(ctx, 16..=65535);
    let max_block_size = noprop::sample_u16_in(ctx, min_block_size..=65535);
    StreamInfo {
        min_block_size,
        max_block_size,
        min_frame_size: noprop::sample_u32_in(ctx, 0..=0xFF_FFFF),
        max_frame_size: noprop::sample_u32_in(ctx, 0..=0xFF_FFFF),
        sample_rate: noprop::sample_u32_in(ctx, 0..=0xF_FFFF),
        channels: noprop::sample_u8_in(ctx, 1..=8),
        bits_per_sample: noprop::sample_u8_in(ctx, 4..=32),
        total_samples: noprop::sample_u64_in(ctx, 0..=0xF_FFFF_FFFF),
        md5: noprop::sample_bytes::<16>(ctx),
    }
}

/// 有効なシークテーブルを生成する
///
/// 通常ポイントはサンプル番号の昇順・一意でなければならないため、
/// 生成した番号をソートして重複を除く。プレースホルダーは末尾にまとめる。
fn seek_table(ctx: &mut noprop::TestCaseContext) -> SeekTable {
    let count = noprop::sample_usize_in(ctx, 0..=32);
    let mut points: Vec<SeekPoint> = (0..count)
        .map(|_| SeekPoint {
            sample_number: noprop::sample_u64_in(ctx, 0..u64::MAX),
            stream_offset: noprop::sample_u64(ctx),
            frame_samples: noprop::sample_u16(ctx),
        })
        .collect();
    // サンプル番号の昇順・一意を構成的に保証する
    points.sort_by_key(|point| point.sample_number);
    points.dedup_by_key(|point| point.sample_number);

    let placeholders = noprop::sample_usize_in(ctx, 0..=3);
    for _ in 0..placeholders {
        points.push(SeekPoint {
            sample_number: SeekPoint::PLACEHOLDER,
            stream_offset: 0,
            frame_samples: 0,
        });
    }
    SeekTable { points }
}

/// APPLICATION ブロックを生成する
fn application(ctx: &mut noprop::TestCaseContext) -> Application {
    let len = noprop::sample_usize_in(ctx, 0..=256);
    Application {
        id: noprop::sample_bytes::<4>(ctx),
        data: noprop::sample_bytes_vec(ctx, len),
    }
}

/// STREAMINFO のラウンドトリップ
#[test]
fn stream_info_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let info = stream_info(ctx);
        let payload = info.encode_payload().expect("エンコードに成功するはず");
        assert_eq!(payload.len(), 34, "STREAMINFO のペイロード長が 34 でない");
        let decoded = StreamInfo::decode(&payload).expect("デコードに成功するはず");
        assert_eq!(decoded, info, "STREAMINFO のラウンドトリップに失敗した");
        Ok(())
    })?;

    Ok(())
}

/// SEEKTABLE のラウンドトリップ
#[test]
fn seek_table_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let table = seek_table(ctx);
        let payload = table.encode_payload().expect("エンコードに成功するはず");
        assert_eq!(
            payload.len(),
            table.points.len() * 18,
            "SEEKTABLE のペイロード長がポイント数 x 18 でない",
        );
        let decoded = SeekTable::decode(&payload).expect("デコードに成功するはず");
        assert_eq!(decoded, table, "SEEKTABLE のラウンドトリップに失敗した");
        Ok(())
    })?;

    Ok(())
}

/// APPLICATION のラウンドトリップ
#[test]
fn application_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let app = application(ctx);
        let payload = app.encode_payload().expect("エンコードに成功するはず");
        let decoded = Application::decode(&payload).expect("デコードに成功するはず");
        assert_eq!(decoded, app, "APPLICATION のラウンドトリップに失敗した");
        Ok(())
    })?;

    Ok(())
}

/// ヘッダー込みのブロックエンコードで last フラグ・タイプ・サイズが正しい
#[test]
fn block_header_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let size = noprop::sample_u32_in(ctx, 0..=1024);
        let is_last = noprop::sample_bool(ctx);

        let block = MetadataBlock::Padding { size };
        let encoded = block.encode(is_last).expect("エンコードに成功するはず");
        assert_eq!(
            encoded.len(),
            4 + size as usize,
            "ブロックのエンコード長がヘッダー 4 バイト + size でない",
        );
        assert_eq!(
            encoded[0] & 0x80 != 0,
            is_last,
            "last-metadata-block フラグが引数と一致しない",
        );
        assert_eq!(encoded[0] & 0x7F, 1, "PADDING のブロックタイプが 1 でない");
        let decoded_size =
            usize::from(encoded[1]) << 16 | usize::from(encoded[2]) << 8 | usize::from(encoded[3]);
        assert_eq!(decoded_size, size as usize, "ブロックサイズが一致しない");
        let decoded = MetadataBlock::decode(1, &encoded[4..]).expect("デコードに成功するはず");
        assert_eq!(
            decoded, block,
            "PADDING ブロックのラウンドトリップに失敗した"
        );
        Ok(())
    })?;

    Ok(())
}
