# FLAC ライブラリ WebAssembly API

FLAC ストリームのデコードとエンコードを行うための WebAssembly API です。

JavaScript/TypeScript から直接呼び出すことができます。

## アーキテクチャ

wasm crate は c-api crate を wasm32-unknown-unknown ターゲット向けにビルドし、wasm 固有の追加機能を提供します。

- **c-api**: デコーダー・エンコーダーの本体（`flac_decoder_*`, `flac_encoder_*` 関数群）
- **wasm**: wasm 固有の追加機能（メモリ管理、JSON シリアライズ等）

## ビルド方法

```bash
# WebAssembly ターゲットをインストール（初回のみ）
rustup target add wasm32-unknown-unknown

# ビルド
cargo build -p wasm --target wasm32-unknown-unknown --profile release-wasm

# 出力ファイル: target/wasm32-unknown-unknown/release-wasm/flac_wasm.wasm
```

### release-wasm プロファイル

`release-wasm` プロファイルはルートの `Cargo.toml` に定義されており、以下の最適化が有効になっています:

- `lto = true`: リンク時最適化
- `codegen-units = 1`: 単一コード生成ユニット
- `opt-level = "z"`: サイズ最適化
- `panic = "abort"`: パニック時に即座に終了
- `strip = true`: シンボル除去

### wasm-opt による最適化

[wasm-opt](https://github.com/WebAssembly/binaryen) (Binaryen) を使用してさらにサイズを最適化できます。

```bash
wasm-opt -Oz --enable-bulk-memory -o flac_wasm.wasm target/wasm32-unknown-unknown/release-wasm/flac_wasm.wasm
```

`--enable-bulk-memory` は `release-wasm` プロファイルが bulk memory 命令を使用するため必要です。

## Examples

- デコードの例:
  - `node crates/wasm/examples/decode.js /path/to/input.flac`
  - FLAC ファイルをデコードして、STREAMINFO とフレーム情報を表示
- エンコードの例:
  - `node crates/wasm/examples/encode.js /path/to/output.flac`
  - サイン波を生成して FLAC ファイルを作成

## 提供する機能

### wasm crate が提供する関数

c-api には含まれない、wasm 固有の追加機能です。

#### メモリ管理

- `flac_alloc`: メモリ確保
- `flac_free`: メモリ解放
- `flac_vec_ptr`: Vec のポインタ取得
- `flac_vec_len`: Vec の長さ取得
- `flac_vec_free`: Vec の解放

#### デコード関連

- `flac_stream_info_to_json`: `flac_decoder_get_stream_info` の結果を JSON に変換
- `flac_decoded_frame_to_json`: `flac_decoder_decode_frame` の結果を JSON に変換

### c-api が提供する関数

c-api の関数がそのまま利用可能です。詳細は `crates/c-api/README.md` を参照してください。

## JSON 形式

### STREAMINFO の例

`flac_stream_info_to_json` 関数の出力です。

```json
{
  "min_block_size": 4096,
  "max_block_size": 4096,
  "min_frame_size": 1201,
  "max_frame_size": 2410,
  "sample_rate": 44100,
  "channels": 2,
  "bits_per_sample": 16,
  "total_samples": 88200,
  "md5": "51def3c1921e494c7f7379ee0adb935e"
}
```

### デコードしたフレームの例

`flac_decoded_frame_to_json` 関数の出力です。

サンプル列そのものは JSON には含まれず、wasm メモリ内の位置（`samples_offset` / `sample_count`）だけが含まれます。
利用側は `Int32Array` などでその位置から直接サンプル列を読み出せます。

```json
{
  "samples_offset": 1183784,
  "sample_count": 8192,
  "sample_rate": 44100,
  "channels": 2,
  "bits_per_sample": 16,
  "first_sample_number": 0
}
```
