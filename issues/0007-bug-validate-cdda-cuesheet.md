# CUESHEET エンコーダーに CD-DA 固有の MUST 制約の検証を追加する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-validate-cdda-cuesheet
- Polished: {YYYY-MM-DD}

## 目的

エンコーダーが RFC 9639 Section 8.7 の CD-DA 固有 MUST 制約に違反する CUESHEET を出力しないよう、検証を追加する。

## 現状

`src/cuesheet.rs` の `Cuesheet::validate_tracks` は `is_cdda` を受け取らず、以下を検証しない（RFC 9639 Section 8.7, 8.7.1, 8.7.1.1 の該当 MUST）：

- トラック数が 100 以下（通常 99 トラック + リードアウト 1 つ）。`Cuesheet::encode_payload` はフォーマット上限の 255 しかチェックしない
- リードアウトのトラック番号が CD-DA では 170、非 CD-DA では 255
- CD-DA の通常トラック番号が 1-99
- インデックスポイント数が 100 以下
- CD-DA のトラックオフセット・インデックスポイントオフセットが 588 で割り切れる

このため、`is_cdda: true` で非準拠のトラック列を持つキューシートがエンコードされてしまう。`pbt/tests/prop_cuesheet.rs` の strategy も `is_cdda: any::<bool>()` で非準拠入力を生成しており、現在のコードはそれをラウンドトリップで受け入れている。

## 設計方針

`Cuesheet::validate_tracks` に `is_cdda` を渡し、CD-DA 時の MUST 制約を検証して `EncodeError::InvalidMetadata` で拒否する。デコーダー側は相互運用性のため寛容なままにする（構造検証は維持）。PBT の strategy を準拠入力のみ生成するよう修正する。

## 完了条件

- 非準拠の CD-DA キューシートが `EncodeError::InvalidMetadata` で拒否されること
- 準拠するキューシートのエンコード → デコードのラウンドトリップが通ること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/cuesheet.rs` の `Cuesheet::validate_tracks` に `is_cdda` を受け渡し、CD-DA 時にトラック数・トラック番号・リードアウト番号・インデックスポイント数・588 整除を検証する
- `Cuesheet::encode_payload` の検証を更新する
- `pbt/tests/prop_cuesheet.rs` の strategy を CD-DA 時に準拠する入力を生成するよう修正する