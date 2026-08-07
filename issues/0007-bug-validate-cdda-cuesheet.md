# CUESHEET エンコーダーに CD-DA 固有の MUST 制約の検証を追加する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/fix-validate-cdda-cuesheet
- Polished: 2026-08-07

## 目的

エンコーダーが RFC 9639 Section 8.7 の CD-DA 固有 MUST 制約に違反する CUESHEET を出力しないよう、検証を追加する。

## 現状

`src/cuesheet.rs` の `validate_tracks` は `is_cdda` を受け取らず、以下を検証しない（RFC 9639 Section 8.7, 8.7.1, 8.7.1.1 の該当 MUST）：

- トラック数が 100 以下（通常 99 トラック + リードアウト 1 つ）。`Cuesheet::encode_payload` はフォーマット上限の 255 しかチェックしない
- リードアウトのトラック番号が CD-DA では 170、非 CD-DA では 255
- CD-DA の通常トラック番号が 1-99
- インデックスポイント数が 100 以下
- CD-DA のトラックオフセット・インデックスポイントオフセットが 588 で割り切れる

このため、`is_cdda: true` で非準拠のトラック列を持つキューシートがエンコードされてしまう。`pbt/tests/prop_cuesheet.rs` の strategy も `is_cdda: any::<bool>()` で非準拠入力を生成しており、現在のコードはそれをラウンドトリップで受け入れている。

## 設計方針

CD-DA 固有の MUST 制約（トラック数・通常トラック番号・リードアウト番号・インデックスポイント数・588 整除）は、エンコーダー（`Cuesheet::encode_payload`）からのみ検証し、`EncodeError::InvalidMetadata` で拒否する。デコーダー（`Cuesheet::decode`）は相互運用性のため従来どおり構造検証のみを維持し、CD-DA 固有の制約は適用しない。リードアウト番号の検証は `is_cdda` に応じて 170 / 255 の両方を扱う。fuzz の「デコードできたブロックは必ず再エンコードできる」前提は、エンコーダー側の CD-DA 制約検証追加により cuesheet では成立しなくなるため、fuzz ターゲット側で対応する。

## 完了条件

- 非準拠の CD-DA キューシート（トラック数 100 超・通常トラック番号 99 超・リードアウト番号 170 以外・インデックスポイント数 100 超・588 非整除）が `EncodeError::InvalidMetadata` で拒否されること
- 非 CD-DA でリードアウト番号が 255 でないキューシートが `EncodeError::InvalidMetadata` で拒否されること
- 準拠するキューシートのエンコード → デコードのラウンドトリップが通ること
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `src/cuesheet.rs` に CD-DA 固有の MUST 制約の検証を追加し、`Cuesheet::encode_payload` からのみ適用する（`validate_tracks` は構造検証のまま維持し、CD-DA 固有制約はデコーダーに適用しない）
- `pbt/tests/prop_cuesheet.rs` の strategy を、`is_cdda` に応じて準拠する入力を生成するよう修正する（CD-DA: 通常トラック 1-99・リードアウト 170・オフセット 588 の倍数、非 CD-DA: リードアウト 255。非 CD-DA の通常トラック番号から 255 を除外する）
- `pbt/tests/prop_cuesheet.proptest-regressions` の非準拠ケースを削除・更新する
- `fuzz/fuzz_targets/fuzz_metadata.rs` の「デコードできたブロックは再エンコードできる」前提を、cuesheet ではエンコーダー側の検証追加により成立しない場合がある形に修正する
- `tests/test_cuesheet.rs` に各拒否パスの単体テストを追加する