# LPC 解析の Levinson-Durbin 法で次数ごとのヒープ確保をスタック配列化する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-stack-alloc-levinson-durbin
- Polished: {YYYY-MM-DD}

## 目的

エンコードの LPC 係数推定（Levinson-Durbin 法）で次数ごとに発生するヒープ確保を削減してエンコードを高速化する。

## 現状

`src/lpc.rs` の `levinson_durbin` は、係数更新の `<Vec<f64>>` を次数ごとに新規確保し、さらに全次数の係数列を `clone` して `results` に保持する。係数は最大 32 個の f64（256 バイト）で、`analyze` 1 回あたりおよそ 20-30 回の小さいヒープ確保になる。ステレオ 4 モードではフレームあたり 4 回の `analyze` が走る。

`analyze` が実際に使うのは `quantize_coefficients` に渡す最良次数の 1 組だけ（`src/lpc.rs` の `analyze` 冒頭の次数選択ループ）で、次数ごとの誤差はすべて必要だが係数列は最良 1 組で足りる。

## 設計方針

`next` と `coefficients` を `[f64; MAX_LPC_ORDER]` のスタック配列 + 長さカウンタに置き換え、最良次数の係数だけを保持する形に変更する。浮動小数点の演算順序・丸めが変わらないようにし、エンコード出力のバイト列を不変に保つ。

## 完了条件

- `analyze` 1 回あたりの次数ごとヒープ確保がなくなること
- エンコード出力のバイト列が変更前と一致すること（`make compare`）
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/lpc.rs` の `levinson_durbin` をスタック配列 + 長さカウンタで書き換える
- 符号量見積もり（`analyze` 内の次数選択ループ）を `levinson_durbin` のループ内へ移し、最良スコアと最良係数のコピー 1 回だけを保持する形に変更する
- `make compare` でバイト列一致を確認する