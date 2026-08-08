# ステレオデコリレーションで非採用プランの残差 Vec をプールへ返却して確保を削減する

- Created: 2026-08-07
- Completed: {YYYY-MM-DD}
- Branch: feature/refactor-recycle-unused-plans
- Polished: 2026-08-09

## 目的

ステレオエンコードで、採用されなかったデコリレーションモードの `Fixed` / `Lpc` プランが所有する残差 `Vec<i64>` を `PlanScratch::residual_pool` に返却し、同じサイズのフレームで残差バッファを再利用できるようにする。

対象は非採用プランの残差バッファであり、エンコーダーが所有するすべての `Vec` の確保・解放をなくすものではない。

## 現状

`src/encoder.rs` の `StreamEncoder::plan_channels` は、ステレオ 2 チャンネルで Independent / LeftSide / SideRight / MidSide の 4 モードを計画し、`SubframePlan` を 4 つ作成するが、合計ビット数が最小となるモードの 2 プランだけを返す。採用された 2 プランは `StreamEncoder::encode_frame` の `SubframePlan::recycle` ループで書き出し後に回収されるが、採用されなかった 2 プランはそのループを通らずドロップされる。非採用プランが `SubframeKind::Fixed` / `SubframeKind::Lpc` の場合、そのプランが所有する残差 `Vec<i64>`（最大ブロックサイズ分）も解放される。

`src/subframe.rs` の `PlanScratch::residual_pool` は残差バッファを計画間・フレーム間で使い回す設計である。`SubframePlan::new` の中で不採用になった Fixed / LPC 候補の残差はすでにプールへ返却されるが、ステレオモード全体で不採用になった最終プランの残差は返却されていない。なお、`SubframePlan::recycle` は `Verbatim` のサンプル `Vec<i64>` も `residual_pool` に入れる一方、`SubframePlan::new` は Verbatim 用に `samples.to_vec()` を作るため、非採用 Verbatim プランへそのまま `recycle` を呼ぶとプールが増え続ける。この経路は今回の回収対象から除外する。

## 設計方針

採用されなかったプランを破棄する前に、`Fixed` / `Lpc` の残差だけを `PlanScratch::residual_pool` へ返す `SubframePlan::recycle_unused` を呼ぶ。`Constant` と `Verbatim` はこの専用経路ではプールへ返さない。採用済みプランに対する既存の `SubframePlan::recycle` の挙動は変更しない。

プールから取り出した残差バッファは `fixed::compute_residual` / `fixed::compute_residual_i32` / `lpc::compute_residual` / `lpc::compute_residual_i32` でクリアまたは resize 後に完全に上書きされるため、採用プランの選択結果を変えずに回収できる。`SubframePlan::recycle` が採用済み `Verbatim` の `Vec` を既存どおり `residual_pool` に返す挙動は今回の変更対象外である。ステレオデコリレーション無効・1 チャンネル・3 チャンネル以上では非採用プランが存在しないため、本変更の回収経路は実行しない（デコリレーションはステレオのみで定義され、非ステレオは Independent のみで符号化する RFC 9639 Section 4.2）。

## 完了条件

- ステレオデコリレーション時、同一サンプル列・同一 `StreamEncoderConfig`・同一ブロックサイズの完全なフレームをウォームアップ後も繰り返し計画したとき、`PlanScratch::take_residual` の pool miss が 0 になり、残差バッファの容量拡張も発生しないことを確認する（プールの本数を 4 本などの固定値と比較しない）
  - pool miss は `take_residual` で `residual_pool` が空だった回数、容量拡張は各 `compute_residual*` 呼び出し前後で対象 `Vec` の `capacity()` が増えた回数と定義し、いずれも累積値のウォームアップ前後の差分で判定する。両者が同時に増える場合もあるため、別カウンターとして記録する
  - `PlanScratch` に test-only の pool miss / 容量拡張カウンターと観測メソッドを追加し、実際に `StreamEncoder::encode_frame` を通るテストから確認できるようにする。採用済みプランの `recycle` まで実行して初めて 1 フレーム分の回収が完了するため、`plan_channels` だけを直接呼ぶテストでは返却された採用プランも明示的に `recycle` する。テスト用カウンターはモックやスタブではなく、残差バッファの再利用経路で発生した事実を数える計測とする
  - `src/subframe.rs` の `#[cfg(test)]` では `SubframePlan::new` 内の候補回収、`recycle_unused` の kind 別挙動、`_i32` を含む残差計算後の容量拡張を確認する。`src/encoder.rs` の `#[cfg(test)]` では `encode_frame` を同一入力で複数回実行し、test-only の観測メソッドで pool miss と容量拡張がウォームアップ後に 0 になることを確認する。`recycle_unused` 経路では非採用 `Verbatim` の `Vec` を `residual_pool` に追加しないことも確認する
- エンコード出力のバイト列が変更前と一致すること（変更前後で同一入力をエンコードし、出力を直接照合して確認）
  - バイト列一致の検証入力は `ChannelAssignment` が Independent / MidSide / LeftSide / SideRight になることを実際に確認できる決定的な信号を含める。信号の性質だけで採用モードを仮定せず、`StreamEncoder::plan_channels` の結果を検証する
  - 採用順序は Independent `[left, right]`、MidSide `[mid, side]`、LeftSide `[left, side]`、SideRight `[side, right]` とし、同点時の選択優先順位 `Independent > MidSide > LeftSide > SideRight` を変更しない
- 変更前の同一基準値と比較して、`make compare` の相互運用・圧縮率・速度の本家比が悪化しないことを確認すること（`make compare` の終了コードだけで判定しない）。フレームデータのバイト数は変更前後で一致し、CLI 速度は同一環境・同一本家バイナリ・同一 release 条件で 3 回の最小値を比較して候補 / 基準が 1.03 以下であること
- 既存テスト・PBT・fuzz がすべて通ること
- 効果が確認できない、または速度が悪化する場合は変更を採用せず、試した内容を `docs/failed-optimizations.md` に記録すること

## 解決方法

- `src/subframe.rs` に、`Fixed` / `Lpc` の残差だけを `residual_pool` へ返し、`Constant` / `Verbatim` は返さない `SubframePlan::recycle_unused` を追加する。採用済みプランの既存 `SubframePlan::recycle` は変更しない
- `src/encoder.rs` の `StreamEncoder::plan_channels` で採用モードを確定した各分岐に、次の順序で非採用プランの `recycle_unused` を追加する。Independent は `side_plan` → `mid_plan` を回収して `[left_plan, right_plan]` を返す。MidSide は `left_plan` → `right_plan` を回収して `[mid_plan, side_plan]` を返す。LeftSide は `right_plan` → `mid_plan` を回収して `[left_plan, side_plan]` を返す。SideRight は `left_plan` → `mid_plan` を回収して `[side_plan, right_plan]` を返す。添字配列を昇順で切り出す実装は行わず、同点時の優先順位も変更しない
- `src/subframe.rs` の単体テストで、非採用回収による `residual_pool` の kind 別挙動、`recycle_unused` 経路での非採用 `Verbatim` の除外、`_i32` を含む残差計算の容量拡張を検証する
- `src/encoder.rs` の単体テストで 4 つの `ChannelAssignment` とサブフレーム順序、同点時の選択優先順位を検証し、`plan_channels` を直接呼ぶ場合は返却された採用プランを `recycle` したうえで test-only の観測メソッドを介して `recycle_unused` の回収結果を確認する。pool miss の実経路は `encode_frame` を使うテストで検証する
- 実装前の基準コミットで決定的な 4 モード分の入力をエンコードして出力バイト列を保存し、実装後の同じ出力と外部比較する。同一テスト内で現行実装を 2 回呼ぶだけの比較にはしない
- 0004 / 0010 が `src/subframe.rs` を変更するため、まずそれらを含めない基準コミットで 0003 の出力・pool miss・ベンチマークを記録する。先に別 issue の変更が入った場合は、その統合後の状態を新しい基準として取り直し、複数 issue の効果を混ぜない。実装前後で同じ基準コミット、Cargo プロファイル、本家 `flac` バイナリを使って `make compare` を実行し、フレームデータのバイト列と CLI 速度を比較する。相互運用の終了コードだけで性能非劣化を判定しない。既存テスト、PBT、fuzz も実行し、性能効果がない場合は変更を採用せず `docs/failed-optimizations.md` に記録する
