# ステレオデコリレーションで非採用プランの残差バッファをプールへ返却して malloc を削減する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-recycle-unused-plans
- Polished: 2026-08-07

## 目的

ステレオエンコードで、採用されなかったデコリレーションモードのプランが残差バッファのプールに返却されず、フレームごとに malloc / free が発生する問題を解消する。

## 現状

`src/encoder.rs` の `StreamEncoder::plan_channels` は、ステレオ 2 チャンネルで Independent / LeftSide / SideRight / MidSide の 4 モードを計画し、`SubframePlan` を 4 つ作成するが、合計ビット数が最小となるモードの 2 プランだけを返す。採用されなかった 2 つのプランは `StreamEncoder::encode_frame` の `SubframePlan::recycle` ループを通らずドロップされ、所有する残差バッファ（`src/subframe.rs` の `SubframeKind::Fixed` / `SubframeKind::Lpc` が保持する `Vec<i64>`、最大ブロックサイズ分）が解放される。

`src/subframe.rs` の `PlanScratch::residual_pool` は「フレームごとの malloc を避けて残差バッファを使い回す」設計だが、ステレオデコリレーション時は毎フレーム最大 2 本の残差 `Vec` が新規確保・解放されるため、プール設計の効果が半分に減殺される。

## 設計方針

採用されなかったプランも破棄する前に `SubframePlan::recycle` を呼び、残差バッファをプールへ返却する。プールから取り出した残差バッファは残差計算で完全に上書きされるため、返却の有無はエンコード結果に影響しない。エンコード出力のバイト列は変わらない（プラン選択結果は不変）。ステレオデコリレーション無効・1 チャンネル・3 チャンネル以上では非採用プランが存在しないため、本変更の影響を受けない。

## 完了条件

- ステレオデコリレーション時、定常状態でフレームごとの残差バッファの確保・解放が発生しないこと（単体テストで確認）
- エンコード出力のバイト列が変更前と一致すること（変更前後で同一入力をエンコードし、出力を直接照合して確認）
- `make compare` で相互運用・圧縮率・速度の本家比が悪化しないことを確認すること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/encoder.rs` の `StreamEncoder::plan_channels` で、採用プラン確定後に非採用プランに対して `SubframePlan::recycle` を呼んで `PlanScratch` に返却する
- 残差バッファのプール再利用を検証する単体テストを追加する