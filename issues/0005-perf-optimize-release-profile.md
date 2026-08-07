# リリースビルドに LTO と codegen-units の最適化設定を追加する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-optimize-release-profile
- Polished: 2026-08-07

## 目的

リリースビルドの最適化設定（LTO / codegen-units）を追加し、エンコード・デコードの実行速度を向上させる。

## 現状

`Cargo.toml` には `[profile.release-wasm]`（サイズ最小化用）のみが定義され、`[profile.release]` が未定義のため、`make compare` や `cargo bench` は lto 無効・codegen-units = 16 のデフォルト設定でビルドされる。lto を有効にすると依存クレート全体をまたぐ最適化（クロスクレート LTO）が働き、エンコード・デコードのホットループ（`src/fixed.rs` の `compute_residual` / `restore_samples`、`src/lpc.rs` の `restore_samples`、`src/rice.rs` の `ResidualPlan::new` / `decode_residual` など）の速度改善が期待できる。効果は `cargo bench` で実測して判断する。

## 設計方針

workspace ルートの `Cargo.toml` に `[profile.release]` を追加する。外部クレートから `shiguredo_flac` を利用する場合、依存側のリリースプロファイルが優先されるため、ライブラリ利用者に設定を強制しない。`target-cpu = "native"` は c-api の配布成果物（staticlib / cdylib）の移植性を壊すため設定しない（release-wasm は release を継承するため設定は波及する）。lto と codegen-units の変更はビルド時間を増やすため、速度向上が確認できた場合のみ設定を維持する。

## 完了条件

- `cargo bench` でエンコード・デコードのスループットが向上することを実測で確認する（エンコード・デコードの全ベンチマーク関数で悪化がないこと。向上の判定は `[profile.release]` の設定変更単独の効果で行う）
- 向上が確認できなかった（同等または悪化）場合は `[profile.release]` の設定を revert し、`docs/failed-optimizations.md` に記録する（CODEBASE.md の規約）
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `Cargo.toml` に `[profile.release]` を追加し、`lto = "thin"` と `codegen-units = 1` を設定する
- 変更前後で `cargo bench` と `make compare` を比較して効果を確認する