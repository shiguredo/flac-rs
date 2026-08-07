# i32 格納版と i64 版のエンコード結果の等価性を検証するテストを追加する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-i32-i64-equivalence-tests
- Polished: 2026-08-07

## 目的

エンコードの i32 格納経路と i64 経路がビット単位で一致することを直接照合するテストを追加し、CODEBASE.md の「形を変えた最適化は変更前後でエンコード出力のバイト列が一致することを確認すること」を回帰保護する。

## 現状

`src/fixed.rs` の `compute_residual_i32` と `src/lpc.rs` の `compute_residual_i32` は、i64 版と同一の演算を i32 格納の widening で行い「結果はビット単位で一致する」とコメントで主張している。`src/fixed.rs` の `best_order_i32` は i32 狭幅の窓内カスケードで「`best_order` と同じ次数を返す」とコメントで主張している。しかし、どれも i64 版と直接照合するテストが存在しない。`src/fixed.rs` の `best_order_matches_direct_reference` は i64 版である `best_order` を二項係数の直接計算と照合するのみで、i32 版との等価性は検証していない。

ロスレス PBT（`pbt/tests/prop_encoder.rs` の `roundtrip_is_lossless` など）は、エンコーダーが同一入力で常に同じ経路（i32 版か i64 版か）を選ぶため、経路間で出力がずれても検出できない。

## 設計方針

i32 版と i64 版の等価性は、i32 版が実際に使われる入力範囲でのみ成立する。`best_order_i32` は `BEST_ORDER_I32_MAX_BITS`（27 bit）以下の入力、`compute_residual_i32` は i32 に収まる入力で等価である。同一入力・同一パラメータで両版を呼び出し、出力が完全一致することを単体テストで照合する。テストは private 関数を直接呼べる `src/fixed.rs` / `src/lpc.rs` のテストモジュールに置く。

## 完了条件

- i32 版と i64 版の出力一致がテストで保証されること（`best_order` の照合は `BEST_ORDER_I32_MAX_BITS` 以下の入力で行う）
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/fixed.rs` のテストモジュールに、`best_order_i32` と `best_order` を `max_order` 0-4 で照合し、`compute_residual_i32` と `compute_residual` を全次数で照合するテストを追加する（`best_order` の照合入力は `BEST_ORDER_I32_MAX_BITS` 以下に制限する）
- `src/lpc.rs` のテストモジュールに、各次数（1-32）で `compute_residual_i32` と `compute_residual` を照合するテストを追加する（係数は量子化精度（15 bit）以内、サンプルは i32 に収まる範囲で生成する。i64 累積のオーバーフローを避けるため）