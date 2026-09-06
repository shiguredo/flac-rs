# ブロッキング戦略の変更検出が機能していない問題を修正する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-blocking-strategy-check
- Polished: 2026-09-07

## 目的

RFC 9639 Section 9.1 の「ブロッキング戦略ビットはストリーム全体を通して変わってはならない（MUST NOT change）」の検証がデッドコードになっており、機能していない問題を修正する。

## 現状

`src/decoder.rs` の `StreamDecoder` は `blocking_strategy: Option<BlockingStrategy>` フィールドを `None` で初期化するが、`Some(...)` への代入がどこにも存在しない。`StreamDecoder::parse_frame` 内の `if let Some(strategy) = self.blocking_strategy` による戦略変更チェックは常に不成立のままである。`parse_frame` は `&self` のため代入も不可能。

RFC 9639 Section 9.1 はブロッキング戦略ビットがストリームを通して変わってはならない（MUST NOT change）と定める。coded number の意味はブロッキング戦略に依存するため（RFC 9639 Appendix B.1）、戦略の一貫性は符号化番号の解釈の前提である。`coded number` の整合チェック（`StreamDecoder::parse_frame`）はフレームが宣言する戦略から期待値を計算するため、新戦略の期待値に合わせた符号化番号を細工したストリームは受け入れられ、MUST 違反を検出できない。

## 設計方針

フレームのデコード成功時に、そのフレームのブロッキング戦略を `StreamDecoder::blocking_strategy` に記録する。2 フレーム目以降の `parse_frame` で既存のチェックが発火するようになる。

## 完了条件

- ストリーム途中でブロッキング戦略が変わる混成戦略ストリーム（それ以外は合法）が `DecodeError::InvalidData` で拒否されること。回帰テストは修正前（戦略チェックがデッドコードの状態）では受理される入力を用いること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/decoder.rs` の `StreamDecoder::decode_frame` の成功パスで `self.blocking_strategy = Some(frame.header.blocking_strategy);` を追加する
- 戦略変更を検出する回帰テストを追加する。テスト入力は、フレーム 1 で記録される戦略と異なる戦略を宣言した 2 フレーム目を含み、かつそれ以外は完全に合法な混成戦略ストリームとする。修正前は受理され、修正後は `DecodeError::InvalidData` で拒否されることを検証する（戦略チェックより前にサブフレーム解析や CRC-16 で失敗すると、修正前後で結果が変わらず回帰テストとして機能しないため）
  - エンコーダー出力と RFC 9639 Appendix D の実例はすべて `Fixed` のため、混成ストリームはエンコーダーで生成した 2 フレーム分のストリームのバイト列を細工して作る。細工は (1) フレームヘッダーのブロッキング戦略ビットを反転し、(2) コード番号を新戦略の期待値（`Variable` ならサンプル数）に合わせ、(3) ヘッダー CRC-8 とフレーム CRC-16 を再計算する
  - コード番号のバイト長を変えるとヘッダー長が変わり後続のサブフレーム位置がずれるため、コード番号が 1 バイトに収まるブロックサイズ（127 以下）を使うこと（例: ブロックサイズ 16 の 2 フレーム目を `Fixed` → `Variable` に反転し、コード番号を 0x01 から 0x10 へ書き換える）
  - CRC の再計算が必要なため、`crate::crc` を利用できる `src/decoder.rs` の `#[cfg(test)]` に置くか、事前計算したバイト列を `tests/helpers/mod.rs` に置いて `tests/test_decoder.rs` から使う