//! picture モジュールの PBT
//!
//! 任意の有効な PICTURE ブロックのラウンドトリップを検証する。

use shiguredo_flac::picture::{Picture, PictureType};

/// テストケース数
const CASES: usize = 128;

/// メディアタイプを生成する: 印字可能 ASCII (RFC 9639 Section 8.8)
fn media_type(ctx: &mut noprop::TestCaseContext) -> String {
    let len = noprop::sample_usize_in(ctx, 0..=32);
    noprop::sample_ascii_printable_string(ctx, len)
}

/// 説明文を生成する (任意の UTF-8)
fn description(ctx: &mut noprop::TestCaseContext) -> String {
    let len = noprop::sample_usize_in(ctx, 0..=32);
    noprop::sample_string(ctx, len)
}

/// 有効な PICTURE ブロックを生成する
fn picture(ctx: &mut noprop::TestCaseContext) -> Picture {
    let data_len = noprop::sample_usize_in(ctx, 0..=256);
    Picture {
        picture_type: PictureType::from_u32(noprop::sample_u32(ctx)),
        media_type: media_type(ctx),
        description: description(ctx),
        width: noprop::sample_u32(ctx),
        height: noprop::sample_u32(ctx),
        color_depth: noprop::sample_u32(ctx),
        colors: noprop::sample_u32(ctx),
        data: noprop::sample_bytes_vec(ctx, data_len),
    }
}

/// PICTURE ブロックのラウンドトリップ
#[test]
fn roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let picture = picture(ctx);
        let payload = picture.encode_payload().expect("エンコードに成功するはず");
        let decoded = Picture::decode(&payload).expect("デコードに成功するはず");
        assert_eq!(decoded, picture, "PICTURE のラウンドトリップに失敗した");
        Ok(())
    })?;

    Ok(())
}

/// 未定義の picture type も含めて u32 との相互変換が一致する
#[test]
fn picture_type_u32_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let value = noprop::sample_u32(ctx);
        assert_eq!(
            PictureType::from_u32(value).to_u32(),
            value,
            "picture type {value} の変換が往復しない",
        );
        Ok(())
    })?;

    Ok(())
}
