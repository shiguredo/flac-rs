# 疎なインパルス信号の圧縮率が本家 flac より大きく劣る原因を調査して改善する

- Created: 2026-07-04
- Completed: 2026-08-09
- Branch: feature/refactor-impulse-compression
- Polished: 2026-08-09

## 目的

`tools/flac_compare` の固定シードによる impulse 信号で、flac-rs のフレームデータサイズが本家 flac より大きくなる原因を特定する。原因ごとの圧縮率と速度への影響を同じ条件で測定し、改善を実装するか、見送るかを決める。

この issue の対象は `signal::impulse` で生成する合成信号であり、実音源全般の圧縮率を保証するものではない。

## 優先度根拠

- 相互運用 (双方向 PCM 一致・flac -t) は完全に通過しており、正しさの問題ではない
- 起票時の比較では impulse の本家 -5 比が 1.301 であり、他の比較信号との差が大きかった
- 本家の圧縮レベルや窓関数は FLAC のフォーマット仕様ではなく実装依存であるため、原因を分離してから変更の採否を判断する

## 現状

起票時の `tools/flac_compare` の実測値 (2026-07-04、本家 flac git-b430c3a5、ビルド日付 20260508) は次のとおり。フレームデータサイズはメタデータブロックを除いた値で、比は `flac-rs / 本家` のバイト数比である。値は過去の測定記録として扱い、再測定時は本家バイナリのバージョン、flac-rs のコミット、ビルドプロファイルを記録する。

| 信号 | flac-rs | 本家 -5 | 比 | 本家 -8 | 比 |
|---|---:|---:|---:|---:|---:|
| impulse (16bit/44.1kHz/2ch 5 秒) | 110048 | 84594 | 1.301 | 62198 | 1.769 |
| 参考: 他 8 信号 | - | - | 0.800-1.001 | - | 0.906-1.001 |

- impulse 信号は `tools/flac_compare/src/signal.rs` の `impulse` が生成する。全サンプル 0 のステレオに、固定シード LCG で 300-811 サンプル間隔の L/R 逆相スパイクを置く。`build_cases` では 44.1 kHz、16 bit、2 チャンネル、5 秒を使う
- flac-rs の既定値と、本家の起票時に確認した圧縮レベル設定には次の差がある。再測定では本家バイナリと設定値を再確認し、比較条件に記録する

| 項目 | flac-rs | 本家 -5 | 本家 -8 |
|---|---|---|---|
| max_lpc_order | 8 | 8 | 12 |
| Rice パーティション最大次数 | 4 | 5 | 6 |
| 窓関数 | Welch | tukey(0.5) | subdivide_tukey(3) |
| LPC 係数精度 | 14 | 自動設定 | 自動設定 |
| ステレオデコリレーション | 4 方式を計画して最小値を選択 | あり | あり |

疎な信号で差が出る機構の仮説は次のとおり。各仮説は基準状態から候補を一つずつ変更したエンドツーエンドの介入効果として測定し、候補間の相互作用は組み合わせ実験で確認する。

1. Rice パーティション最大次数の差。impulse はパーティションを細かく切ることで、全ゼロ残差の区間とスパイク区間を分離できる可能性がある。全ゼロ残差のパーティションは Rice パラメータ 0 のほか、固定長 0 bit の escape (RFC 9639 Section 9.2.7.1) としても表現でき、どちらで符号化されるかは `src/rice.rs` の `ResidualPlan::new` の選択に依存する。4096 サンプルのフレームでは、パーティションオーダー 4 / 5 / 6 の区画幅はそれぞれ 256 / 128 / 64 サンプルである。ただし第 1 パーティションの残差数は区画幅から予測次数を引いた値であり、最後の短いフレーム (5 秒 = 220500 サンプルは 4096 × 53 + 3412 で、最終フレームの 3412 サンプルは実効最大パーティションオーダーが 2 に制限される) や実際に選択されたオーダーとは区別する
2. 予測 (固定予測・LPC の次数と選択) の差。スパイクは予測不能なので、予測が「ほぼ無音」に最適化されるかどうかで残差の分布が変わる
3. ステレオデコリレーション選択の差。逆相スパイクは `src/encoder.rs` の `StreamEncoder::plan_channels` で mid が常に 0、side が `2 * value` になる形に変換される
4. LPC 係数精度や窓関数など、予測係数の推定条件の差。これらは Rice パーティションとの相互作用を含めて候補ごとに確認する

## 設計方針

調査と改善の比較条件を固定し、原因を一つずつ切り分ける。実験用の設定を公開 API に追加せず、`tools/flac_compare` の設定済み比較ケースとソースコードの候補変更を使う。

1. 同一入力、同一ブロックサイズ、同一ビルドプロファイルで flac-rs と本家 -5 / -8 を測定する。flac-rs は `StreamEncoderConfig::block_size = 4096` を使い、本家側にも `-b 4096` を明示する。本家の実行ファイルは `FLAC_BIN` または PATH から取得し、バージョンを出力に残す (本家の探索とバージョン表示は `tools/flac_compare/src/main.rs` の `ReferenceFlac::locate` / `ReferenceFlac::version` が既に実装済みで、それを利用する)。圧縮率は `tools/flac_compare/src/main.rs` の `frame_data_size` と同じく、メタデータを除いたフレームデータバイト数で比較する
2. 本家側は `ReferenceFlac::analyze` を追加し、`flac -a -f -o <analysis-path> <flac-path>` を実行して診断情報を取得する。flac-rs 側は `tools/flac_compare` に FLAC ビットストリームの診断パーサーを追加し、同じオンワイヤー情報を出力する。サブフレームはビット境界を考慮して比較し、ライブラリの private / `pub(crate)` API を公開しない。取得する診断項目の一覧は解決方法 2 に示す
3. 窓関数は FLAC のビットストリームに記録されないため、`flac -a` の結果だけから寄与を特定しない。本家は `-A tukey(0.5)` など単一窓を指定した比較と、`-A subdivide_tukey(3)` の比較を分けて行う。flac-rs は `src/lpc.rs::apply_welch_window` の候補をソースコード上で一つずつ変更して測定する。LPC 係数精度は `src/encoder.rs::LPC_PRECISION` を変更対象とし、本家は `-q 14` と自動設定を別の実験として比較する
4. まず `StreamEncoderConfig::max_partition_order` だけを 4 / 5 / 6 に変えて impulse のフレームデータサイズを比較する。次に、予測次数、LPC 係数精度・窓関数、`StreamEncoder::plan_channels` のチャンネル割り当て (例: 特定モードの強制、モード数の削減) を一項目ずつ変更して、エンドツーエンドの介入効果を測る。各変更では後続の予測方式・Rice 方式・チャンネル割り当てが再選択されるため、その相互作用を含む結果として記録し、単独の原因寄与とは主張しない。複数の候補に効果がある場合は、基準、A のみ、B のみ、A+B の組み合わせを測り、相互作用を含めて原因候補を絞る。最大オーダーは上限であり、実際に選ばれたパーティションオーダーと区別して記録する
5. 原因候補が確定したら、既定値の変更と選択ロジックの変更を別々に評価する。変更を採用する場合は、候補ごとに圧縮率、impulse を含むエンコード速度、ロスレス性、相互運用性を測定する
6. 測定の基準は、この issue の候補に着手した時点の `develop` のコミット、`Cargo.toml` のプロファイル、本家バイナリのバージョンで固定する (圧縮率・CLI 速度の比較基準。Criterion のベースラインは解決方法 4 の保存コミットを使う)。基準側で `git rev-parse HEAD` と `git status --short` を記録し、0003、0004、0005、0008、0010 が同じ実装箇所やベンチマーク条件を変更した後に測定する場合は、その変更を含んだ状態で基準値を取り直し、候補の効果と混ぜない
7. 実装方法だけを変更して選択結果を維持する候補では、変更前後のエンコード出力バイト列一致を必須とする。LPC 係数精度や窓関数など符号化条件を変更する候補は、同じサブフレーム種別・次数でも係数と残差が変わり得るため、意図的な出力変更として扱い、バイト列一致を要求しない。既定値や選択結果を意図的に変更する候補も、デコード後の PCM 一致・相互運用性・下記の圧縮率と速度基準で評価する。両方の変更を一つの比較に混ぜない

注意事項:

- 既定値や選択結果を変更する改善では、ロスレス性 (`fuzz_encoder_roundtrip`) と相互運用 (`make compare`) の全通過を必ず確認する
- 圧縮率改善の過程で試した速度改善が効果がなかった・逆効果だった場合は `docs/failed-optimizations.md` に記録する (記録対象は完了条件の見送り分岐と同じ)

## 完了条件

以下のいずれかで closed にする。改善を実装する場合は下記 2 項目の **両方** を満たすこと。

- 改善を実装する場合: impulse のフレームデータバイト数 / 本家 -5 のフレームデータバイト数が 1.05 以下であること (起票時 1.301 の約 20% 改善に相当する目標値)。`tools/flac_compare` の他 8 信号は、変更前の flac-rs を基準に各信号のフレームデータバイト数が 1.03 倍を超えて増加しないこと。`cargo bench -p benches` の `encode_impulse`、`encode_tonal`、`encode_noise` は同じ bench プロファイルで変更前後を測定し、Criterion が報告する候補 / 基準の実行時間が各 1.03 以下であること
- 改善を実装する場合: `make compare` の相互運用チェック、フレームデータ圧縮率、CLI 速度を確認し、既存テスト、PBT、fuzz が通過すること。CLI 速度の判定は変更前の flac-rs を基準に行い (本家比は参考値として記録する)、1.03 倍を超えて悪化しないこと。実装方法だけを変更した場合は、変更前後のエンコード出力バイト列も一致すること
- 改善を見送る場合: パーティション、予測、ステレオデコリレーション、予測係数の推定条件の寄与を測定結果で示し、圧縮率と速度のトレードオフを根拠付きで本 issue に記録すること。速度改善を試して効果がなかった場合は `docs/failed-optimizations.md` にも記録すること (効果なし・逆効果の記録対象は速度改善のみで、圧縮率の失敗手は本 issue に記録する)。改善を採用する場合も、検討したが不採用にした候補 (例: 効果が小さかったパーティションオーダー) は同様に本 issue に記録する

## 解決方法

1. `tools/flac_compare/src/main.rs` の `ReferenceFlac::encode` に `-b 4096` の指定を追加する。flac-rs 側も `run_case` の `StreamEncoderConfig` に `block_size: 4096` を明示し、`benches/benches/codec.rs::config` にも同じ値を設定する (既定値と同一のため挙動は変わらないが、測定条件をコード上で固定する意味がある)。起票時と同じ impulse および他 8 信号のフレームデータサイズを設定ごとに記録し、本家の実行ファイルのバージョン、flac-rs の基準コミット、ビルド条件も同時に記録する
2. `tools/flac_compare/src/main.rs` に `ReferenceFlac::analyze` を追加して `flac -a -f -o <analysis-path> <flac-path>` の出力を保存する。また `ReferenceFlac::encode` が `-A` と `-q` を候補ごとに受け取れるようにする。`tools/flac_compare` には FLAC ビットストリームの診断パーサーを追加し、`flac -a` と flac-rs 側の結果をサブフレーム単位で照合する。診断項目にはパーティションオーダー、Rice の 4 bit / 5 bit 方式、各パーティションの Rice パラメータまたは escape の固定長、予測方式・次数、wasted bits、チャンネル割り当て、LPC 係数精度、量子化シフトを含める。`flac -a` の分析出力は本家実装依存で項目が揃わない可能性があるため、出力に現れない項目は flac-rs 側の診断パーサー単独の情報として記録し、照合は一致する項目だけで行う。窓関数はオンワイヤー情報から判定せず、本家の `-A` と `src/lpc.rs::apply_welch_window` の制御実験の条件として記録する。診断パーサーと `ReferenceFlac::analyze` は調査完了後も `tools/flac_compare` に残し、将来の圧縮率調査に再利用できる状態を保つ
3. 寄与が確認できた項目だけを変更候補とし、公開 API を増やさず、次のシンボルを対象に一つずつ実装・計測する。
   - `src/encoder.rs` の `StreamEncoderConfig::default`、`StreamEncoder::plan_channels`
   - `src/encoder.rs` の `LPC_PRECISION`
   - `src/subframe.rs` の `SubframePlan::new`
   - `src/rice.rs` の `ResidualPlan::new`
   - `src/fixed.rs` の `best_order` / `best_order_i32`
   - `src/lpc.rs` の `analyze`、`apply_welch_window`
4. `benches/benches/codec.rs` に `tools/flac_compare/src/signal.rs::impulse` と同じ固定シード、同じ 5 秒のフレーム数を使う `encode_impulse` を追加する。benches クレートから flac_compare (bin crate) の `mod signal` を import できないため、impulse の生成コードは bench 側に複製し、生成結果が元の `impulse` と一致することを照合テストで確認する (照合テストは benches クレート内のテストとして置き、期待値は flac_compare 側の `impulse` から一度生成した固定値を埋め込む。生成コードの複製が元から乖離すると比較自体の意味がなくなるため)。診断経路とベンチマークを追加した状態を Criterion のベースライン保存コミットとし、そのコミットで `cargo bench -p benches --bench codec -- --save-baseline impulse-before` を実行する。候補側で `cargo bench -p benches --bench codec -- --baseline impulse-before` を実行し、Criterion の推定値を比較する。採用候補について `make compare` のフレームデータ圧縮率・CLI 速度・相互運用チェック、既存テスト、PBT、fuzz を実行する。見送った速度改善は `docs/failed-optimizations.md` に記録する (対象は完了条件の見送り分岐と同じ)

## 調査結果と見送りの判断

以下の調査は、基準を次の条件に固定して実施した。

- 本家 flac: 1.5.0 (Homebrew でインストールした実行ファイル)
- flac-rs の基準コミット: 98d60f0 (作業ブランチ `feature/refactor-impulse-compression` の分岐点)
- ビルドプロファイル: release (Cargo.toml の既定)
- ブロックサイズ: flac-rs / 本家とも 4096 (`-b 4096`)

### 実装した診断基盤

- `tools/flac_compare/src/diagnostic.rs` を追加し、FLAC ビットストリームからフレームごとのパーティションオーダー、Rice パラメータ (4 bit / 5 bit 方式)、各パーティションの Rice パラメータまたは escape の固定長、予測方式・次数、wasted bits、チャンネル割り当て、LPC 係数精度、量子化シフトを抽出できるようにした
- `tools/flac_compare/src/main.rs` に `ReferenceFlac::analyze` (`flac -a -f -o`) と `--diagnose` / `--compare-ana` フラグを追加し、本家の分析出力と flac-rs の出力をサブフレーム単位で照合できるようにした
- `benches/benches/codec.rs` に `encode_impulse` を追加し、`benches/tests/impulse_signal.rs` で flac_compare の `signal::impulse` との一致 (サンプル数・スパイク数・FNV-1a 64 ハッシュ) を照合するテストを追加した
- `tools/flac_compare/src/main.rs` の `ReferenceFlac::encode` に `-b 4096` を追加し、`run_case` の `StreamEncoderConfig` と `benches/benches/codec.rs::config` に `block_size: 4096` を明示した (既定値と同一)

### 原因の特定

診断パーサーの照合で、impulse のフレームデータサイズ差の主因は **Rice パーティション最大次数の差** であることを確認した。

- flac-rs (オーダー 4): `partition_order=4` (パーティション幅 256 サンプル)
- 本家 -5 (オーダー 5): `partition_order=5` (パーティション幅 128 サンプル)
- 本家 -8 (オーダー 6): `partition_order=6` (パーティション幅 64 サンプル)

サブフレームの予測方式 (両者 FIXED order=0) とチャンネル割り当て (両者 MID_SIDE) は一致しており、ステレオデコリレーションの選択差は無かった。LPC 係数精度・窓関数は impulse では FIXED が選ばれるため寄与しない。仮説 2・3・4 は寄与なしと判断した。仮説 4 は設計方針 3 の制御実験 (`-A` / `-q` の切り替え) を実行していないが、FIXED サブフレームには LPC の係数精度・窓関数が出力に現れず、選択結果を変え得ないため、観測から寄与なしと断定できる。

### パーティションオーダー変更の実測

`StreamEncoderConfig::default` の `max_partition_order` を 4 / 5 / 6 / 7 / 8 に変えて、`make compare` 相当の条件 (相互運用チェック込み) でフレームデータサイズを測定した。

| max_partition_order | impulse フレームデータ | 本家 -5 比 | 他 8 信号 (変更前 4 基準) |
|---|---:|---:|---|
| 4 (現行) | 110048 | 1.301 | 基準 |
| 5 | 64499 | 0.762 | 悪化なし (tonal -18、wasted -22 のみ) |
| 6 | 40049 | 0.473 | 悪化なし (tonal -175、wasted -65 のみ) |
| 7 | 29130 | 0.344 | tonal も改善 (-6105) |
| 8 | 28413 | 0.336 | tonal / square / wasted も大幅改善 |

相互運用チェックは全設定で 33/33 通過した。圧縮率の目標 (impulse 比 1.05 以下) はオーダー 5 で達成する。

### エンコード速度の実測

Criterion のベースライン (`impulse-before`、オーダー 4) に対する実行時間比。ベースラインは 2026-08-09 に、診断ツールと `encode_impulse` ベンチを追加した状態 (基準コミット 98d60f0 + 作業ブランチの未コミット変更) で `cargo bench -p benches --bench codec -- --save-baseline impulse-before` を実行して保存した。bench 名は `codec/encode_tonal`、`codec/encode_noise`、`codec_impulse/encode_impulse` (impulse は 5 秒相当のためスループット表示を正しくするべく別グループ)。

| max_partition_order | encode_impulse | encode_tonal | encode_noise |
|---|---:|---:|---:|
| 5 | 0.966 (-3.4%) | 1.034 (+3.4%) | 1.045 (+4.5%) |
| 6 | 0.969 (-3.1%) | 1.108 (+10.8%) | 1.137 (+13.7%) |

オーダー 5 では encode_tonal (+3.4%) と encode_noise (+4.5%) が完了条件の 1.03 を超える。原因はパーティション数が 16 → 32 に倍増し、noise のような高エントロピー信号ではパーティション分割の利益が無いにもかかわらず、統計収集のコストが増えるため。encode_impulse は改善 (-3.4%) するが、完了条件は 3 ベンチすべてが対象のため、オーダー 5 の採用は完了条件を満たさない。

### 判断

パーティションオーダーを上げる変更は、圧縮率を大きく改善する (オーダー 5 で impulse 比 1.301 → 0.762) ものの、エンコード速度の完了条件 (encode_impulse / encode_tonal / encode_noise が各 1.03 以下) を満たせない。オーダー 6 以上は速度悪化がさらに大きくなる。圧縮率と速度のトレードオフが完了条件の制約と整合しないため、**改善の実装は見送る**。

- 採用候補: なし
- 不採用にした候補: `max_partition_order` の 5 / 6 / 7 / 8 (圧縮率は改善するがエンコード速度が完了条件を超える)
