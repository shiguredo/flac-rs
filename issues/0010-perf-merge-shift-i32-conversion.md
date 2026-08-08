# wasted bits が 0 でない経路のシフトと samples_i32 変換を 1 ループに融合してパスを減らす

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-merge-shift-i32-loop
- Polished: 2026-08-09

## 目的

wasted bits が 0 でないサブフレーム計画で、シフト処理と `samples_i32` 変換が別ループで走る 2 パス構造を 1 ループに融合し、エンコードを高速化する。

## 現状

`src/subframe.rs` の `SubframePlan::new` は、wasted bits が 0 でないとき (CONSTANT 判定を通過した場合) `shifted = samples.iter().map(|&s| s >> wasted_bits).collect()` でシフト済みサンプル列を作り、その後 `samples_i32.extend(samples.iter().map(|&s| s as i32))` で i32 変換を別ループで行う。この経路は 2 パスの変換になっている。i32 変換は変数シャドーイングによりシフト済みサンプル列を変換対象とする。

docs/failed-optimizations.md の共通の罠には「ループ内の push / extend_from_slice は容量チェックがブロック全体の自動ベクトル化を殺す。事前に長さを確定した resize 済みスライスへ直接書き込むこと」と記録されている。

## 設計方針

シフトと `samples_i32` 変換を 1 ループに融合し、シフト結果を i64 側と i32 側の両方に書き込む。`resize` 済みスライスへの直接書き込みで実装する (docs/failed-optimizations.md の実測方針に従う)。

wasted bits が 0 でないときは coded_bits <= 32 になるため、非 CONSTANT 経路では `samples_i32` は常に作られ、融合は wasted bits が 0 でない分岐内で追加の条件分岐なしに適用できる (根拠: RFC 9639 Section 9.1.4 の最大 32 bits per sample、Section 9.2.3 のサイドチャンネル +1 でサブフレーム上限 33、Section 9.2.2 の wasted bits は結果ビット深度が正になる MUST)。wasted bits が 0 の経路の既存 i32 変換ループは変更しない。算術は変更しないためエンコード出力のバイト列は不変。

## 完了条件

- 融合の効果を、wasted bits が 0 でない（かつ CONSTANT でない）信号を追加した `make bench` (`cargo bench -p benches`) で実測する。Criterion の `--save-baseline` / `--baseline` で変更前後を比較し、候補 / 基準の実行時間が 1.03 倍以内かつ信頼区間が重ならないことを効果の判定基準とする。効果が確認できない（同等または悪化）場合は融合の変更を revert して `docs/failed-optimizations.md` に記録すること（追加したベンチ信号は将来の最適化の実測にも使えるため残す）。基準はベンチ信号の追加を先にコミットして `--save-baseline` を実行し、その後融合を実装して `--baseline` で比較する（変更前のデータが存在しないと比較不能になるため）
- エンコード出力のバイト列が変更前と一致すること（wasted bits が 0 でない信号を検証入力に含め、変更前後で同一入力をエンコードして出力を直接照合して確認）
- `make compare` で相互運用・圧縮率・速度の本家比が悪化しないことを確認すること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/subframe.rs` の `SubframePlan::new` で、シフトと `samples_i32` 変換を 1 ループに融合する（`resize` 済みスライスへの直接書き込みで実装する）
- 融合の i64 側書き込み先は 0004 (シフト済みバッファの PlanScratch 再利用) と同一領域のため、0004 が先に統合された場合は `PlanScratch::shifted` を使い、先に入らなかった場合はローカル `Vec` で実装して 0004 統合時に移し替える。0004 の効果 (確保削減) と本 issue の効果 (パス削減) を混ぜないため、0004 を含めた状態で基準値を取り直す。0003 も `src/subframe.rs` を変更するため、同様に統合後の状態で基準値を取り直す
- wasted bits が 0 でない信号を `benches/benches/codec.rs` に追加する。複製した信号の生成コードは bench ターゲットから lib ターゲットが import できないため `benches/src/lib.rs` に置き、`benches/benches/codec.rs` から参照する。`tools/flac_compare/src/signal.rs` の `wasted` と同一の信号を複製し、生成結果が一致することを照合テストで確認する。照合テストは `benches/src/lib.rs` の `#[cfg(test)]` に置き、期待値は flac_compare 側の `wasted` から一度生成した固定値を埋め込む（flac_compare は bin-only クレートのため import できず、生成コードの複製が元から乖離すると実測の意味がなくなるため。照合は数フレーム分の短いサンプル列で行い、リテラル量を抑える）。`wasted` は tonal ベースで LPC 解析が支配的なため 2 パスの寄与が埋もれないよう、固定予測が主役の信号（`tools/flac_compare/src/signal.rs` の `square` は振幅が下位 4 bit が 0 の値で wasted bits 4 の信号として使える）もベンチに追加する