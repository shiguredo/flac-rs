# wasted bits が 0 でない経路のシフトと samples_i32 変換を 1 ループに融合してパスを減らす

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-merge-shift-i32-loop
- Polished: {YYYY-MM-DD}

## 目的

wasted bits が 0 でないサブフレーム計画で、シフト処理と `samples_i32` 変換が別ループで走る 2 パス構造を 1 ループに融合し、エンコードを高速化する。

## 現状

`src/subframe.rs` の `SubframePlan::new` は、wasted bits が 0 でないとき `shifted = samples.iter().map(|&s| s >> wasted_bits).collect()` でシフト済みサンプル列を作り、その後 `samples_i32.extend(samples.iter().map(|&s| s as i32))` で i32 変換を別ループで行う。この経路は 2 パスの変換になっている。

docs/failed-optimizations.md には「ループ内の push / extend_from_slice は容量チェックがブロック全体の自動ベクトル化を殺す。事前に長さを確定した resize 済みスライスへ直接書き込むこと」と実測方針が記録されており、1 ループ化には resize 済みスライスへの直接書き込みを使う。

## 設計方針

シフトと `samples_i32` 変換を 1 ループに融合し、シフト結果を i64 側と i32 側の両方に書き込む。wasted bits が 0 でないときは coded_bits <= 32 になるため `samples_i32` は常に作られ、融合は条件分岐なく適用できる。算術は変更しないためエンコード出力のバイト列は不変。

## 完了条件

- 融合の効果を、wasted bits が 0 でない（かつ CONSTANT でない）信号を追加した `cargo bench` で実測し、効果が確認できない場合は `docs/failed-optimizations.md` に記録すること
- エンコード出力のバイト列が変更前と一致すること（変更前後で同一入力をエンコードし、出力を直接照合して確認）
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/subframe.rs` の `SubframePlan::new` で、シフトと `samples_i32` 変換を 1 ループに融合する（`resize` 済みスライスへの直接書き込みで実装する）
- wasted bits が 0 でない信号を `benches/benches/codec.rs` に追加する