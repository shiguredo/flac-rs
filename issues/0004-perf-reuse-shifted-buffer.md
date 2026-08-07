# wasted bits 対応のシフト済みサンプルバッファを PlanScratch で再利用する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-reuse-shifted-buffer
- Polished: 2026-08-07

## 目的

wasted bits が 0 でないサブフレーム計画で、シフト済みサンプル列の `Vec` が毎回 malloc されるのをやめ、作業バッファの再利用でエンコードを高速化する。

## 現状

`src/subframe.rs` の `SubframePlan::new` は、wasted bits が 0 でないとき（CONSTANT 判定を通過した場合）`samples.iter().map(|&s| s >> wasted_bits).collect()` でブロックサイズ分の `Vec<i64>`（既定ブロックサイズ 4096 で 32 KiB）を毎回確保する。ステレオ 4 モードでは 1 フレームあたり最大 4 回発生する。

`PlanScratch` は `samples_i32` / `rice` / `windowed` / `residual_pool` を保持して計画ごとの malloc を避ける設計だが、シフト済みサンプル用のバッファが含まれておらず、この確保は再利用の対象から漏れている。

## 設計方針

`PlanScratch` にシフト済みサンプル用の `Vec<i64>` を追加し、`resize` + スライス直接書き込みで使い回す（docs/failed-optimizations.md の実測方針に従う）。算術は変更しないためエンコード出力のバイト列は不変。

## 完了条件

- wasted bits が 0 でない信号で、定常状態（2 フレーム目以降）でのフレームごとのシフト済みサンプルバッファ確保が発生しないこと（単体テストで確認）
- エンコード出力のバイト列が変更前と一致すること（変更前後で同一入力をエンコードし、出力を直接照合して確認）
- `make compare` で相互運用・圧縮率・速度の本家比が悪化しないことを確認すること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/subframe.rs` の `PlanScratch` に `shifted: Vec<i64>` を追加する
- `SubframePlan::new` で `shifted` を `resize` + スライス直接書き込みで使い回す
- シフト済みサンプルバッファの再利用を検証する単体テストを追加する