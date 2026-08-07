# wasted bits 対応のシフト済みサンプルバッファを PlanScratch で再利用する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-reuse-shifted-buffer
- Polished: {YYYY-MM-DD}

## 目的

wasted bits が 0 でないサブフレーム計画で、シフト済みサンプル列の `Vec` が毎回 malloc されるのをやめ、作業バッファの再利用でエンコードを高速化する。

## 現状

`src/subframe.rs` の `SubframePlan::new` は、wasted bits が 0 でないとき `samples.iter().map(|&s| s >> wasted_bits).collect()` でブロックサイズ分の `Vec<i64>`（既定ブロックサイズ 4096 で 32 KB）を毎回確保する。ステレオ 4 モードでは 1 フレームあたり最大 4 回発生する。

`PlanScratch` は `samples_i32` / `rice` / `windowed` / `residual_pool` を保持して計画ごとの malloc を避ける設計だが、シフト済みサンプル用のバッファが含まれておらず、この確保は再利用の対象から漏れている。さらに、シフト済みサンプルからの `samples_i32` 変換が別ループで走るため、この経路は 2 パスの変換になっている。

## 設計方針

`PlanScratch` にシフト済みサンプル用の `Vec<i64>` を追加し、`clear` + `extend` で使い回す。併せて、シフト処理と `samples_i32` 変換を 1 ループに融合してパスを減らす。算術は変更しないためエンコード出力のバイト列は不変。

## 完了条件

- wasted bits が 0 でない信号で、フレームごとのシフト済みサンプルバッファ確保が発生しないこと
- エンコード出力のバイト列が変更前と一致すること（`make compare`）
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/subframe.rs` の `PlanScratch` に `shifted: Vec<i64>` を追加する
- `SubframePlan::new` で `samples_i32` と同様に `clear` + `extend`（または `resize` + スライス直接書き込み）で使い回す
- シフトと `samples_i32` 変換を 1 ループに融合する