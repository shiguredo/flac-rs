# 疎なインパルス信号の圧縮率が本家 flac より大きく劣る原因を調査して改善する

- Priority: Medium
- Created: 2026-07-04
- Completed: 2026-07-08
- Model: Fable 5
- Branch: feature/update-impulse-compression
- Polished: 2026-07-04
## 目的

疎なインパルス列 (ほぼ無音 + 時折スパイク) で、flac-rs のフレームデータサイズが本家 flac より 30% (-5 比) 〜 77% (-8 比) 大きい。打楽器・クリック音・無音の多い素材など現実に存在する信号クラスでの系統的な劣化なので、原因を特定し、圧縮率と速度のトレードオフを評価したうえで改善の採否を決める。

## 優先度根拠

Medium とする。

- 相互運用 (双方向 PCM 一致・flac -t) は完全に通過しており、正しさの問題ではない
- 圧縮率の総合値は本家 -5 比 0.990 と同等であり、劣化は impulse に局所的
- 一方で -5 比 1.301 は、既知の弱点だった矩形波 (-8 比 1.40、-e + apodization 差) と異なり本家の標準設定に対する差であり、信号クラスとして系統的なので放置しない

## 現状

tools/flac_compare (`make compare`) の実測 (2026-07-04、本家 flac git-b430c3a5 20260508)。フレームデータサイズはメタデータブロックを除いた値。

| 信号 | flac-rs | 本家 -5 | 比 | 本家 -8 | 比 |
|---|---:|---:|---:|---:|---:|
| impulse (16bit/44.1kHz/2ch 5 秒) | 110048 | 84594 | 1.301 | 62198 | 1.769 |
| 参考: 他 8 信号 | - | - | 0.800-1.001 | - | 0.906-1.001 |

- impulse 信号の定義: tools/flac_compare/src/signal.rs の impulse()。全サンプル 0 のステレオに 300-811 サンプル間隔 (固定シード LCG) で L/R 逆相のスパイクを置いたもの
- エンコード設定の既知の差 (flac-rs: src/encoder.rs の StreamEncoderConfig::default、本家: src/libFLAC/stream_encoder.c の compression_levels_):

| 項目 | flac-rs | 本家 -5 | 本家 -8 |
|---|---|---|---|
| max_lpc_order | 8 | 8 | 12 |
| Rice パーティション最大次数 | 4 | 5 | 6 |
| 窓関数 | Welch | tukey(0.5) | subdivide_tukey(3) |
| mid-side | 試す | あり | あり |

疎な信号で差が出る機構の仮説 (調査で検証する):

1. Rice パーティション最大次数の差。impulse はパーティションを細かく切るほど無音区間 (k=0) とスパイク区間を分離でき利得が大きい。block 4096 で flac-rs は最小 256 サンプル / パーティションまで、本家 -5 は 128、-8 は 64 まで細分できる
2. 予測 (固定予測・LPC の次数と選択) の差。スパイクは予測不能なので、予測が「ほぼ無音」に最適化されるかどうかで残差の分布が変わる
3. ステレオデコリレーション選択の差。逆相スパイクは mid がゼロ・side がスパイクになる形

## 設計方針

調査を先行し、原因を確定してから改善を実装する。

1. 同一入力に対する flac-rs と本家 -5 の出力をサブフレーム単位で突き合わせ、ビット差の内訳 (サブフレームタイプ・予測次数・wasted bits・Rice パーティション次数・パラメータ) を特定する。本家側の内訳は flac -a (analyze) で取得できる
2. 仮説 1 の即検証として、StreamEncoderConfig の max_partition_order を 5 / 6 に上げたときの出力サイズを測る (公開フィールドなので設定変更だけで測れる)
3. 原因確定後、改善の圧縮率効果と速度コストを benches と make compare で評価し、デフォルト値の変更で済むのか選択ロジックの変更が必要なのかを切り分けて採否を決める

注意事項:

- 改善はエンコード出力のバイト列が変わる系。ロスレス性 (fuzz_encoder_roundtrip) と相互運用 (make compare) の全通過を必ず確認する
- 効果がなかった・逆効果だった手は docs/failed-optimizations.md に記録する

## 完了条件

以下のいずれかで closed にする。

- 改善を実装する場合: impulse の本家 -5 比が 1.05 以下 (目安) になり、他 8 信号の圧縮率とエンコード速度 (encode_tonal / encode_noise) に 3% を超える劣化がないこと
- 改善を見送る場合: 劣化の主因と見送りの根拠 (圧縮率と速度のトレードオフ) を本 issue に記録すること

## 解決方法

(調査完了後に記載する)

変更候補 (調査で絞り込む):

- src/encoder.rs: StreamEncoderConfig::default の max_partition_order
- src/rice.rs: パーティション次数の探索
- src/fixed.rs / src/lpc.rs: 次数選択
- src/subframe.rs: サブフレーム計画・ステレオデコリレーション選択
