# FLAC ライブラリ C API

FLAC ストリームのデコードとエンコードを行うための C 言語 API です。

C 言語用のヘッダファイルは [`include/flac.h`](./include/flac.h) にあり、
以下のサンプルプログラムに実際の使用例が記載されています:

- [`examples/decode.c`](./examples/decode.c): FLAC ファイルをチャンクごとに投入しながらデコードして情報を表示する例
- [`examples/encode.c`](./examples/encode.c): サイン波を生成して FLAC ファイルにエンコードする例

## サンプルプログラムのビルド方法

```bash
# flac-rs のプロジェクトルートでライブラリをビルド
cargo build --release

# サンプルプログラムのビルドに必要なファイルのパスは以下の通りです:
# - C ヘッダファイル: c-api/include/flac.h
# - ライブラリファイル:
#   - target/release/libflac.a (静的ライブラリ)
#   - target/release/libflac.dylib (動的ライブラリ、Linux では libflac.so)

# decode.c をビルドおよび実行
cc -o target/release/decode \
   -I c-api/include/ \
   c-api/examples/decode.c \
   target/release/libflac.a
./target/release/decode /path/to/input.flac

# encode.c をビルドおよび実行
cc -o target/release/encode \
   -I c-api/include/ \
   c-api/examples/encode.c \
   target/release/libflac.a -lm
./target/release/encode /path/to/output.flac
```

> [!NOTE]
> ライブラリのファイル名 (`libflac`) はリファレンス実装の `libFLAC` と大文字小文字だけが異なります。
> macOS などの大文字小文字を区別しないファイルシステムで両方を同じディレクトリに配置すると衝突するため、
> 併用する場合は配置ディレクトリを分けてください。

## 提供する機能

### 共通

- `flac_library_version`: ライブラリのバージョン取得

### デコード

- `flac_decoder_new`: デコーダー作成
- `flac_decoder_free`: デコーダー解放
- `flac_decoder_get_last_error`: エラーメッセージ取得
- `flac_decoder_feed`: 入力データ投入
- `flac_decoder_finish`: 入力の終端通知
- `flac_decoder_decode_frame`: フレームのデコード
- `flac_decoder_get_stream_info`: STREAMINFO 取得
- `flac_decoder_get_vorbis_comment`: VORBIS_COMMENT (タグ) 取得

### エンコード

- `flac_encoder_new`: エンコーダー作成
- `flac_encoder_free`: エンコーダー解放
- `flac_encoder_get_last_error`: エラーメッセージ取得
- `flac_encoder_set_sample_rate`: サンプルレート設定
- `flac_encoder_set_channels`: チャンネル数設定
- `flac_encoder_set_bits_per_sample`: ビット深度設定
- `flac_encoder_set_block_size`: ブロックサイズ設定
- `flac_encoder_set_max_lpc_order`: LPC 最大次数設定
- `flac_encoder_set_max_partition_order`: Rice パーティション最大オーダー設定
- `flac_encoder_set_stereo_decorrelation`: ステレオデコリレーション設定
- `flac_encoder_set_vorbis_comment`: VORBIS_COMMENT (タグ) 設定
- `flac_encoder_initialize`: 初期化
- `flac_encoder_push_samples`: サンプル投入
- `flac_encoder_finalize`: 完了処理
- `flac_encoder_get_output`: 完成した FLAC ストリームの取得

## メタデータ (VORBIS_COMMENT) の扱い

VORBIS_COMMENT は人間可読なタグ (TITLE / ARTIST など) を保持するメタデータブロックです (RFC 9639 Section 8.6)。

### エンコード時の設定

`flac_encoder_set_vorbis_comment()` で設定します。**`flac_encoder_initialize()` より前に呼ぶ必要があります**。

```c
const char *names[] = {"TITLE", "ARTIST"};
const char *values[] = {"My Song", "My Artist"};
flac_encoder_set_vorbis_comment(encoder, "my-app", names, values, 2);
```

渡した文字列は関数の中で複製されるため、呼び出し後に解放して構いません。

### デコード時の取得

`flac_decoder_get_vorbis_comment()` で取得します。
メタデータブロックを読み終える必要があるため、**最初のフレームをデコードした後**に呼んでください。

```c
FlacDecodedFrame frame;
while ((ret = flac_decoder_decode_frame(decoder, &frame)) == FLAC_ERROR_OK) {
    if (!comment_printed) {
        FlacVorbisComment comment;
        if (flac_decoder_get_vorbis_comment(decoder, &comment) == FLAC_ERROR_OK) {
            printf("vendor: %s\n", comment.vendor);
            for (uint32_t i = 0; i < comment.field_count; i++) {
                printf("%s=%s\n", comment.fields[i].name, comment.fields[i].value);
            }
            comment_printed = 1;
        }
    }
    // frame を処理する...
}
```

ストリームに VORBIS_COMMENT が含まれていない場合は `FLAC_ERROR_NOT_FOUND` が返ります。

`comment.vendor` と `comment.fields` が参照するバッファはデコーダーが所有しています。
次のいずれかを行うと無効になるため、必要な内容は呼び出し側で複製してください。

- 同じデコーダーに対してもう一度 `flac_decoder_get_vorbis_comment()` を呼ぶ
- `flac_decoder_free()` を呼ぶ
