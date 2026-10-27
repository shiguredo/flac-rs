//! cuesheet モジュールの PBT
//!
//! 任意の有効なキューシートのラウンドトリップを検証する。

use shiguredo_flac::cuesheet::{Cuesheet, CuesheetTrack, CuesheetTrackIndex};

/// テストケース数
const CASES: usize = 128;

/// メディアカタログ番号を生成する: 印字可能 ASCII の右 0x00 パディング
/// (RFC 9639 Section 8.7)
fn media_catalog_number(ctx: &mut noprop::TestCaseContext) -> [u8; 128] {
    let len = noprop::sample_usize_in(ctx, 0..=128);
    let mut mcn = [0u8; 128];
    for byte in mcn.iter_mut().take(len) {
        *byte = noprop::sample_u8_in(ctx, 0x20..=0x7E);
    }
    mcn
}

/// トラック 1 個分の中身 (番号以外) を生成する
///
/// インデックスポイント番号は 0 または 1 から始まる連番でなければならない
/// (RFC 9639 Section 8.7.1.1)。リードアウト以外は 1 個以上必要なので
/// 1..=4 個生成する。
fn track_body(ctx: &mut noprop::TestCaseContext) -> CuesheetTrack {
    let offset_samples = noprop::sample_u64(ctx);
    let isrc = noprop::sample_bytes::<12>(ctx);
    let is_non_audio = noprop::sample_bool(ctx);
    let pre_emphasis = noprop::sample_bool(ctx);
    let first_number = noprop::sample_u8_in(ctx, 0..=1);
    let count = noprop::sample_usize_in(ctx, 1..=4);
    let index_points = (0..count)
        .map(|i| CuesheetTrackIndex {
            offset_samples: noprop::sample_u64(ctx),
            number: first_number + i as u8,
        })
        .collect();
    CuesheetTrack {
        offset_samples,
        // 呼び出し側で一意な番号を割り当てる
        number: 0,
        isrc,
        is_non_audio,
        pre_emphasis,
        index_points,
    }
}

/// 有効なキューシートを生成する
///
/// トラック番号は一意 (RFC 9639 Section 8.7.1)、最終トラックはリードアウトで
/// インデックスポイントを持たない (RFC 9639 Section 8.7.1)。
fn cuesheet(ctx: &mut noprop::TestCaseContext) -> Cuesheet {
    let media_catalog_number = media_catalog_number(ctx);
    let lead_in_samples = noprop::sample_u64(ctx);
    let is_cdda = noprop::sample_bool(ctx);

    // トラック数は 1..=8。最小 1 / 最大 8 を境界値として確実に踏む
    let count =
        noprop::sample_with_boundaries(ctx, &[1usize, 4, 8], noprop::Ratio::new(1, 2), |ctx| {
            noprop::sample_usize_in(ctx, 1..=8)
        });
    // トラック番号は一意 (RFC 9639 Section 8.7) のため、重複なしの集合を
    // 有効回数で選ぶ (1..=255 からの重複なし抽出は 1 試行で約 9 割成功する)
    let numbers = noprop::sample_with_rejection(ctx, 64, |ctx| {
        let set: std::collections::BTreeSet<u8> = (0..count)
            .map(|_| noprop::sample_u8_in(ctx, 1..=255))
            .collect();
        (set.len() == count).then_some(set)
    });
    let mut tracks: Vec<CuesheetTrack> = numbers
        .into_iter()
        .map(|number| {
            let mut body = track_body(ctx);
            body.number = number;
            body
        })
        .collect();

    // 最終トラックはリードアウトに置き換える (インデックスポイントなし)
    let last_number = tracks[count - 1].number;
    tracks[count - 1] = CuesheetTrack {
        offset_samples: noprop::sample_u64(ctx),
        number: last_number,
        isrc: noprop::sample_bytes::<12>(ctx),
        is_non_audio: false,
        pre_emphasis: false,
        index_points: Vec::new(),
    };

    Cuesheet {
        media_catalog_number,
        lead_in_samples,
        is_cdda,
        tracks,
    }
}

/// キューシートのラウンドトリップ
#[test]
fn roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("FLAC_PBT_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let cuesheet = cuesheet(ctx);
        let payload = cuesheet.encode_payload().expect("エンコードに成功するはず");
        let decoded = Cuesheet::decode(&payload).expect("デコードに成功するはず");
        assert_eq!(decoded, cuesheet, "CUESHEET のラウンドトリップに失敗した");
        Ok(())
    })?;

    Ok(())
}
