# リリースビルドに LTO と codegen-units の最適化設定を追加する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-optimize-release-profile
- Polished: {YYYY-MM-DD}

## 目的

リリースビルドの最適化設定（LTO / codegen-units）を追加し、エンコード・デコードの実行速度を向上させる。

## 現状

`Cargo.toml` には `[profile.release-wasm]`（サイズ最小化用）のみが定義され、`[profile.release]` が未定義のため、`make compare` や `cargo bench` は lto 無効・codegen-units = 16 のデフォルト設定でビルドされる。ホット関数（`src/fixed.rs` の `compute_residual` / `restore_samples`、`src/lpc.rs` の `restore_samples` / `compute_residual_i32_with_order`、`src/rice.rs` の `ResidualPlan::new` / `decode_residual` など）に `#[inline]` も付いていないため、codegen-units = 16 ではクロス CGU のインライン展開が制限され、数 %〜10% 前後の速度余地があると見込まれる。

## 設計方針

workspace ルートの `Cargo.toml` に `[profile.release]` を追加する。外部クレートから `shiguredo_flac` を利用する場合、依存側のリリースプロファイルが優先されるため、ライブラリ利用者に設定を強制しない。`target-cpu = "native"` は c-api の配布成果物（staticlib / cdylib）と wasm32 ビルドに波及して移植性を壊すため設定しない。

## 完了条件

- `cargo bench` でエンコード・デコードのスループットが向上（または同等）することを実測で確認する
- 効果が確認できない場合は `docs/failed-optimizations.md` に記録する（CODEBASE.md の規約）
- 既存テスト・PBT・fuzz がすべて通ること

## 解決方法

- `Cargo.toml` に `[profile.release]` を追加し、`lto = "thin"` と `codegen-units = 1` を設定する
- 必要に応じてホット関数に `#[inline]` を付与する
- 変更前後で `cargo bench` と `make compare` を比較して効果を確認する