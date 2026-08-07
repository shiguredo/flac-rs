# ブロッキング戦略の変更検出が機能していない問題を修正する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-blocking-strategy-check
- Polished: 2026-08-07

## 目的

RFC 9639 Section 9.1 の「ブロッキング戦略ビットはストリーム全体を通して変わってはならない（MUST NOT change）」の検証がデッドコードになっており、機能していない問題を修正する。

## 現状

`src/decoder.rs` の `StreamDecoder` は `blocking_strategy: Option<BlockingStrategy>` フィールドを `None` で初期化するが、`Some(...)` への代入がどこにも存在しない。`StreamDecoder::parse_frame` 内の `if let Some(strategy) = self.blocking_strategy` による戦略変更チェックは常に不成立のままである。`parse_frame` は `&self` のため代入も不可能。

RFC 9639 Section 9.1 はブロッキング戦略ビットがストリームを通して変わってはならない（MUST NOT change）と定める。coded number の意味はブロッキング戦略に依存するため（RFC 9639 Appendix B.1）、戦略の一貫性は符号化番号の解釈の前提である。`coded number` の整合チェック（`StreamDecoder::parse_frame`）はフレームが宣言する戦略から期待値を計算するため、新戦略の期待値に合わせた符号化番号を細工したストリームは受け入れられ、MUST 違反を検出できない。

## 設計方針

フレームのデコード成功時に、そのフレームのブロッキング戦略を `StreamDecoder::blocking_strategy` に記録する。2 フレーム目以降の `parse_frame` で既存のチェックが発火するようになる。

## 完了条件

- ストリーム途中でブロッキング戦略が変わる入力が `DecodeError::InvalidData` で拒否されること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/decoder.rs` の `StreamDecoder::decode_frame` の成功パスで `self.blocking_strategy = Some(frame.header.blocking_strategy);` を追加する
- 戦略変更を検出する回帰テストを追加する（coded number チェックを素通りするよう、新戦略の期待値に合わせた符号化番号を細工した混成戦略ストリームを使う）