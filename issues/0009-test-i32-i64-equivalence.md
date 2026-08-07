# i32 格納版と i64 版のエンコード結果の等価性を検証するテストを追加する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-i32-i64-equivalence-tests
- Polished: {YYYY-MM-DD}

## 目的

エンコードの i32 格納経路と i64 経路がビット単位で一致することを直接照合するテストを追加し、CODEBASE.md の「形を変えた最適化は変更前後でエンコード出力のバイト列が一致することを確認すること」を回帰保護する。

## 現状

`src/fixed.rs` の `best_order_i32` / `compute_residual_i32` と `src/lpc.rs` の `compute_residual_i32` は、i64 版と同一の演算を i32 格納の widening で行い「結果はビット単位で一致する」とコメントで主張している。しかし、両者を直接照合するテストが存在しない。`src/fixed.rs` の `best_order_matches_direct_reference` は i64 版である `best_order` を二項係数の直接計算と照合するのみで、i32 版との等価性は検証していない。

`pbt/tests/prop_encoder.rs` の `streaming_push_matches_oneshot` は入力の分割方法に対する決定性を検証するが、エンコーダーは同一入力で常に同じ経路を選ぶため、経路間で出力がずれてもロスレス PBT は通ってしまう。

## 設計方針

同一入力・同一パラメータで i32 版と i64 版を呼び出し、出力が完全一致することを単体テストで照合する。`src/fixed.rs` と `src/lpc.rs` のテストモジュールに追加する。

## 完了条件

- i32 版と i64 版の出力一致がテストで保証されること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/fixed.rs` のテストモジュールに、全次数で `best_order_i32 == best_order` と `compute_residual_i32 == compute_residual` を照合するテストを追加する
- `src/lpc.rs` のテストモジュールに、各次数で `compute_residual_i32 == compute_residual` を照合するテストを追加する