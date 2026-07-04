# flac-rs

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![GitHub Actions](https://github.com/shiguredo/flac-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/shiguredo/flac-rs/actions/workflows/ci.yml)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/shiguredo)

## About Shiguredo's open source software

We will not respond to PRs or issues that have not been discussed on Discord. Also, Discord is only available in Japanese.

Please read <https://github.com/shiguredo/oss> before use.

## 時雨堂のオープンソースソフトウェアについて

利用前に <https://github.com/shiguredo/oss> をお読みください。

## 概要

Rust で実装された依存 0 かつ Sans I/O な FLAC (Free Lossless Audio Codec, RFC 9639) ライブラリです。

> [!WARNING]
> 本ライブラリは実験的なライブラリです。正式リリースされることはありません。

## 特徴

- Sans I/O
  - <https://sans-io.readthedocs.io/index.html>
- no_std 対応
  - <https://docs.rust-embedded.org/book/intro/no-std.html>
- 依存ライブラリ 0
- RFC 9639 準拠
  - デコード: CONSTANT / VERBATIM / FIXED / LPC サブフレーム、全ステレオデコリレーション、CRC-8 / CRC-16 / MD5 検証
  - エンコード: ロスレス保証 (エンコード → デコードで元のサンプル列と完全一致)
  - メタデータ: STREAMINFO / PADDING / APPLICATION / SEEKTABLE / VORBIS_COMMENT / CUESHEET / PICTURE
- C API (crates/c-api)
- WebAssembly API (crates/wasm)

## 性能

正しさと堅牢性を最優先としつつ、safe Rust の範囲で以下の高速化を実施しています。

- 自動ベクトル化される形へ整えることによる SIMD 化 (arm64 / x86_64、intrinsics / unsafe 不使用)
- ビット I/O のワード単位化 (64 bit アキュムレータ、キャッシュ付きリフィル)
- CRC-8 / CRC-16 のテーブル駆動化 (slice-by-8) と MD5 の分岐なしループ化
- Rice パラメータ選択の 1 パス統計化 (選択結果は全探索と同一)
- LPC / 固定予測の次数別特殊化と、i32 格納 + i64 widening の積和 (結果は i64 算術とビット単位で一致)
- 作業バッファの再利用によるアロケーション削減

60 秒 16 bit ステレオ信号の CLI end-to-end 比較 (Apple Silicon) では、リファレンス実装の flac -5 に対してエンコードは約 1.5 倍の時間、デコードは約 1.4 倍高速で、圧縮率はほぼ同等です。`make compare` で本家 flac コマンドとの相互運用、圧縮率、速度を一括で確認できます。

## 使い方

### デコード

```rust
use shiguredo_flac::decoder::StreamDecoder;

let mut decoder = StreamDecoder::new();
decoder.feed(&flac_bytes);
decoder.finish();

while let Some(frame) = decoder.decode_frame()? {
    // frame.samples はチャンネルインターリーブ済みのサンプル列
    println!("decoded {} samples", frame.samples.len());
}
```

### エンコード

```rust
use shiguredo_flac::encoder::{StreamEncoder, StreamEncoderConfig};

let config = StreamEncoderConfig {
    sample_rate: 44100,
    channels: 2,
    bits_per_sample: 16,
    ..Default::default()
};
let mut encoder = StreamEncoder::new(config)?;
encoder.push_samples(&samples)?; // チャンネルインターリーブ済みのサンプル列
let flac_bytes = encoder.finish()?;
```

## C API

C 言語から利用するためのバインディング ([crates/c-api](./crates/c-api)) を提供しています。

`flac_decoder_*` / `flac_encoder_*` 関数群で、Sans I/O なデコード・エンコードを C からそのまま利用できます。
C ヘッダファイルは cbindgen で自動生成されます。

```bash
# ライブラリのビルド
cargo build --release

# 成果物:
# - C ヘッダファイル: crates/c-api/include/flac.h
# - 静的ライブラリ: target/release/libflac.a
# - 動的ライブラリ: target/release/libflac.dylib (Linux では libflac.so)
```

サンプルは [decode.c](./crates/c-api/examples/decode.c) / [encode.c](./crates/c-api/examples/encode.c) にあります。
ビルド方法など詳細は [crates/c-api/README.md](./crates/c-api/README.md) を参照してください。

## WebAssembly API

JavaScript/TypeScript から利用するための WebAssembly バインディング ([crates/wasm](./crates/wasm)) を提供しています。

C API の全関数に加えて、wasm 固有のメモリ管理関数 (`flac_alloc` など) と
JSON 変換関数 (`flac_stream_info_to_json` / `flac_decoded_frame_to_json`) を提供します。

```bash
# WebAssembly ターゲットをインストール (初回のみ)
rustup target add wasm32-unknown-unknown

# ビルド (成果物: target/wasm32-unknown-unknown/release-wasm/flac_wasm.wasm)
cargo build -p wasm --target wasm32-unknown-unknown --profile release-wasm
```

Node.js のサンプルは [decode.js](./crates/wasm/examples/decode.js) / [encode.js](./crates/wasm/examples/encode.js) にあります。
詳細は [crates/wasm/README.md](./crates/wasm/README.md) を参照してください。

## ライセンス

Apache License 2.0

```text
Copyright 2026-2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
