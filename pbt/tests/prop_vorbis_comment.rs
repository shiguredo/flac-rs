//! vorbis_comment モジュールの PBT
//!
//! 任意の有効なフィールド名・値でのラウンドトリップを検証する。

use shiguredo_flac::vorbis_comment::{VorbisComment, VorbisCommentField};

/// テストケース数
const CASES: usize = 128;

/// 有効なフィールド名を生成する: U+0020 から U+007E (U+003D の = を除く)
/// (RFC 9639 Section 8.6)
fn field_name(ctx: &mut noprop::TestCaseContext) -> String {
    let len = noprop::sample_usize_in(ctx, 1..=16);
    (0..len)
        .map(|_| {
            loop {
                let b = noprop::sample_u8_in(ctx, 0x20..=0x7E);
                // `=` は名前と値の区切り文字なので名前には使えない
                if b != 0x3D {
                    break b as char;
                }
            }
        })
        .collect()
}

/// フィールド値を生成する (任意の UTF-8 文字列。= を含んでもよい)
fn field_value(ctx: &mut noprop::TestCaseContext) -> String {
    let len = noprop::sample_usize_in(ctx, 0..=32);
    noprop::sample_string(ctx, len)
}

/// 有効な VORBIS_COMMENT を生成する
fn vorbis_comment(ctx: &mut noprop::TestCaseContext) -> VorbisComment {
    let vendor_len = noprop::sample_usize_in(ctx, 0..=32);
    let vendor = noprop::sample_string(ctx, vendor_len);
    let count = noprop::sample_usize_in(ctx, 0..=8);
    let fields = (0..count)
        .map(|_| VorbisCommentField {
            name: field_name(ctx),
            value: field_value(ctx),
        })
        .collect();
    VorbisComment { vendor, fields }
}

/// VORBIS_COMMENT のラウンドトリップ
#[test]
fn roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let comment = vorbis_comment(ctx);
        let payload = comment.encode_payload().expect("エンコードに成功するはず");
        let decoded = VorbisComment::decode(&payload).expect("デコードに成功するはず");
        assert_eq!(
            decoded, comment,
            "VORBIS_COMMENT のラウンドトリップに失敗した",
        );
        Ok(())
    })?;

    Ok(())
}
