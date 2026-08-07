# LPC 解析の Levinson-Durbin 法で次数ごとのヒープ確保をスタック配列化する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-stack-alloc-levinson-durbin
- Polished: 2026-08-07

## 目的

エンコードの LPC 係数推定（Levinson-Durbin 法）で次数ごとに発生するヒープ確保を削減してエンコードを高速化する。

## 現状

`src/lpc.rs` の `levinson_durbin` は、係数更新の `Vec<f64>` を次数ごとに新規確保し、さらに全次数の係数列を `clone` して `results` に保持する。係数は最大 32 個の f64（256 バイト）で、`analyze` 1 回あたり次数ごとの小さいヒープ確保が多数（既定の最大次数 8 で十数回以上）発生する。ステレオ 4 モードではフレームあたり 4 回の `analyze` が走る。

`analyze` が実際に使うのは `quantize_coefficients` に渡す最良次数の 1 組だけ（`src/lpc.rs` の `analyze` の次数選択ループ）で、次数ごとの誤差はすべて必要だが係数列は最良 1 組で足りる。

## 設計方針

`next` と `coefficients` を `[f64; MAX_LPC_ORDER]` のスタック配列 + 長さカウンタに置き換え、最良次数の係数だけを保持する形に変更する。全次数の候補をスタックに保持すると 8 KB 規模になるため、次数選択を `levinson_durbin` のループ内へ融合して最良 1 組だけを持つ。浮動小数点の演算順序・丸めが変わらないようにし、エンコード出力のバイト列を不変に保つ。符号量見積もりは現在の `analyze` と同じく、各次数の誤差の更新後に計算する。

## 完了条件

- `analyze` 1 回あたりの次数ごとヒープ確保がなくなること（コード検査で確認）
- エンコード出力のバイト列が変更前と一致すること（変更前後で同一入力をエンコードし、出力を直接照合して確認。docs/failed-optimizations.md の規約どおり、LPC 経路を通る高予測ゲイン信号（tonal 等）を検証入力に含める）
- `make compare` で相互運用・圧縮率・速度の本家比が悪化しないことを確認すること
- `cargo bench` でエンコード速度が向上することを実測し、効果が確認できない（同等または悪化）場合は変更を revert し、`docs/failed-optimizations.md` に記録すること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/lpc.rs` の `levinson_durbin` を `[f64; MAX_LPC_ORDER]` のスタック配列 + 長さカウンタで書き換える
- 符号量見積もり（`analyze` 内の次数選択ループ）を `levinson_durbin` のループ内へ移し、最良スコアと最良係数の 1 組だけを保持する形に変更する（`block_size` と `precision` を `levinson_durbin` に渡すシグネチャ変更を伴う。見積もりは誤差の更新後に計算し、現在の `analyze` と同一の次数選択になるようにする）
- 変更前後で `cargo bench` と `make compare` を比較して効果を確認する