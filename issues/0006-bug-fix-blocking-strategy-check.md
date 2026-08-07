# ブロッキング戦略の変更検出が機能していない問題を修正する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-blocking-strategy-check
- Polished: {YYYY-MM-DD}

## 目的

RFC 9639 Section 9.1 の「ブロッキング戦略ビットはストリーム全体を通して変わってはならない（MUST NOT change）」の検証がデッドコードになっており、機能していない問題を修正する。

## 現状

`src/decoder.rs` の `StreamDecoder` は `blocking_strategy: Option<BlockingStrategy>` フィールドを `None` で初期化するが、`Some(...)` への代入がどこにも存在しない。`StreamDecoder::parse_frame` 内の `if let Some(strategy) = self.blocking_strategy` による戦略変更チェックは常に不成立のままである。`parse_frame` は `&self` のため代入も不可能。

RFC 9639 Section 9.1「A decoder that does not check for such a change could be vulnerable to buffer overflows」の意図を満たせていない。`coded number` の整合チェック（`StreamDecoder::parse_frame`）が偶然拾うケースはあるが、両戦略で一致する符号化番号を細工したストリームは受け入れられる。

## 設計方針

フレームのデコード成功時に、そのフレームのブロッキング戦略を `StreamDecoder::blocking_strategy` に記録する。2 フレーム目以降の `parse_frame` で既存のチェックが発火するようになる。

## 完了条件

- ストリーム途中でブロッキング戦略が変わる入力が `ParseError::Invalid` で拒否されること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/decoder.rs` の `StreamDecoder::decode_frame` の成功パスで `self.blocking_strategy = Some(frame.header.blocking_strategy);` を追加する