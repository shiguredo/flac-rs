# ステレオデコリレーションで非採用プランの残差バッファをプールへ返却して malloc を削減する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-recycle-unused-plans
- Polished: {YYYY-MM-DD}

## 目的

ステレオエンコードで、採用されなかったデコリレーションモードのプランが残差バッファのプールに返却されず、フレームごとに malloc / free が発生する問題を解消する。

## 現状

`src/encoder.rs` の `StreamEncoder::plan_channels` は、ステレオ 2 チャンネルで Independent / LeftSide / SideRight / MidSide の 4 モードを計画し、`SubframePlan` を 4 つ作成するが、合計ビット数が最小の 2 つだけを返す。採用されなかった 2 つのプランは `StreamEncoder::encode_frame` の `SubframePlan::recycle` ループを通らずドロップされ、所有する残差バッファ（`src/subframe.rs` の `SubframeKind::Fixed` / `SubframeKind::Lpc` が保持する `Vec<i64>`、最大ブロックサイズ分）が解放される。

`src/subframe.rs` の `PlanScratch::residual_pool` は「フレームごとの malloc を避けて残差バッファを使い回す」設計だが、ステレオ時は毎フレーム最大 2 本の残差 `Vec` が新規確保・解放されるため、プール設計の効果が半分に減殺される。

## 設計方針

採用されなかったプランも破棄する前に `SubframePlan::recycle` を呼び、残差バッファをプールへ返却する。エンコード出力のバイト列は変わらない（プラン選択結果は不変）。

## 完了条件

- ステレオエンコードでフレームごとの残差バッファ順「malloc / free」が発生しないこと
- エンコード出力のバイト列が変更前と一致すること（`make compare`）
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/encoder.rs` の `StreamEncoder::plan_channels` で、採用プラン確定後に非採用プランに対して `SubframePlan::recycle` を呼んで `PlanScratch` に返却する
- または `StreamEncoder::encode_frame` 側で返却対象を拡張する