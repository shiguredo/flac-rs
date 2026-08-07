# デコーダーの MD5 計算を STREAMINFO の MD5 が不明なときにスキップして高速化する

- Created: 2026-08-07
- Completed: 2026-08-07
- Branch: feature/refactor-skip-md5-when-unknown
- Polished: 2026-08-07

## 目的

デコード時に無条件で実行される MD5 計算を、STREAMINFO の MD5 が全ゼロ（不明）のときは省略してデコードを高速化する。

## 現状

`src/decoder.rs` の `StreamDecoder::decode_frame` はフレームのデコード成功パスで毎回 `StreamDecoder::update_md5` を呼ぶ。`update_md5` は `src/md5.rs` の `samples_to_md5_bytes`（サンプル列のバイト列変換 1 パス）と `Md5::update`（ダイジェスト計算 1 パス）を実行する。

一方、`StreamDecoder::verify_end` は `info.md5 != [0u8; 16]` のときしか MD5 を照合しない。RFC 9639 Section 8.2 は MD5 が全ゼロの値は「不明」を表すと定める（"A value of 0 signifies that the value is not known."）。そのため MD5 がゼロのストリームでは、毎フレームの MD5 変換とダイジェスト計算は照合に使われることはなく、常に死重となる。

## 設計方針

MD5 の照合が必要かどうかはストリームごとに決まるため、メタデータフェーズで一度だけ判定する。`StreamDecoder` に `md5_known` フラグを追加し、フレームフェーズ突入時に `stream_info.md5 != [0u8; 16]` を判定してキャッシュする。`decode_frame` の成功パスはフラグが立っているときだけ `update_md5` を呼ぶ形に変更する。

MD5 が非ゼロのストリームでは従来どおり計算・照合されるため、`Md5Mismatch` の検出挙動は変わらない。デコード結果のサンプル列も不変。

## 完了条件

- MD5 が全ゼロのストリームがデコード成功し、`update_md5` が一度も呼ばれないこと（単体テストで確認）
- MD5 が非ゼロのストリームのデコード結果と MD5 照合が従来どおり動作すること
- 既存テスト・PBT・fuzz がすべて通ること
- `make compare` でデコード速度の本家比が悪化しないことを確認すること

## 解決方法

- `src/decoder.rs` の `StreamDecoder` に `md5_known: bool` フィールドを追加する
- STREAMINFO メタデータブロックを処理した時点で `info.md5 != [0u8; 16]` を判定して設定する
- `StreamDecoder::decode_frame` の成功パスを `if self.md5_known { self.update_md5(&frame); }` に変更する
- 全ゼロ MD5 ストリームでスキップされることを検証する単体テストを追加する
- `make compare` で相互運用 33 項目全通過、デコード速度の本家比 x0.94 で悪化なし