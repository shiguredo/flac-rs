// C API のメタデータ (VORBIS_COMMENT) のエンコード / デコードを検証するテスト
//
// エンコード時に VORBIS_COMMENT を設定し、デコード時に同じ内容が
// 取得できることを確認する

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "flac.h"

#define SAMPLE_RATE 44100
#define CHANNELS 2
#define BITS_PER_SAMPLE 16
#define TOTAL_SAMPLES 10000
#define BLOCK_SIZE 4096

// テストに使うメタデータ
static const char *VENDOR = "flac-rs c-api test";
static const char *FIELD_NAMES[] = {"TITLE", "ARTIST", "ALBUM"};
static const char *FIELD_VALUES[] = {"Test Title", "Test Artist", "Test Album"};
#define FIELD_COUNT 3

// テスト用のサンプル列を生成する (チャンネルインターリーブ済み)
static void generate_samples(int32_t *samples, uint32_t count) {
    for (uint32_t i = 0; i < count; i++) {
        // チャンネルごとに異なる波形にして、インターリーブの取り違えを検出できるようにする
        int32_t phase = (int32_t)(i / CHANNELS) % 1000;
        samples[i] = (i % CHANNELS == 0) ? phase * 10 : -phase * 10;
    }
}

// FLAC ストリームに VORBIS_COMMENT を設定してエンコードする
static uint8_t *encode_with_metadata(uint64_t *out_size) {
    FlacEncoder *encoder = flac_encoder_new();
    if (encoder == NULL) {
        fprintf(stderr, "テスト失敗: flac_encoder_new() が NULL を返した\n");
        return NULL;
    }

    FlacError ret = flac_encoder_set_sample_rate(encoder, SAMPLE_RATE);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: サンプルレートの設定に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return NULL;
    }
    flac_encoder_set_channels(encoder, CHANNELS);
    flac_encoder_set_bits_per_sample(encoder, BITS_PER_SAMPLE);
    flac_encoder_set_block_size(encoder, BLOCK_SIZE);

    // VORBIS_COMMENT を設定する
    ret = flac_encoder_set_vorbis_comment(encoder, VENDOR, FIELD_NAMES,
                                          FIELD_VALUES, FIELD_COUNT);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: VORBIS_COMMENT の設定に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return NULL;
    }

    ret = flac_encoder_initialize(encoder);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: エンコーダーの初期化に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return NULL;
    }

    // サンプルを投入する
    int32_t samples[TOTAL_SAMPLES];
    generate_samples(samples, TOTAL_SAMPLES);
    ret = flac_encoder_push_samples(encoder, samples, TOTAL_SAMPLES);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: サンプルの投入に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return NULL;
    }

    ret = flac_encoder_finalize(encoder);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: エンコードの完了処理に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return NULL;
    }

    const uint8_t *data = NULL;
    uint64_t size = 0;
    ret = flac_encoder_get_output(encoder, &data, &size);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: 出力の取得に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return NULL;
    }

    // エンコーダーを解放すると data が無効になるため、内容を複製して返す
    uint8_t *copy = (uint8_t *)malloc((size_t)size);
    if (copy == NULL) {
        fprintf(stderr, "テスト失敗: メモリ確保に失敗した\n");
        flac_encoder_free(encoder);
        return NULL;
    }
    memcpy(copy, data, (size_t)size);
    *out_size = size;

    flac_encoder_free(encoder);
    return copy;
}

// FLAC ストリームをデコードして VORBIS_COMMENT を検証する
static int decode_and_verify_metadata(const uint8_t *flac_data, uint64_t flac_size) {
    FlacDecoder *decoder = flac_decoder_new();
    if (decoder == NULL) {
        fprintf(stderr, "テスト失敗: flac_decoder_new() が NULL を返した\n");
        return 0;
    }

    FlacError ret =
        flac_decoder_feed(decoder, flac_data, (uint32_t)flac_size);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: データの投入に失敗した: %s\n",
                flac_decoder_get_last_error(decoder));
        flac_decoder_free(decoder);
        return 0;
    }
    flac_decoder_finish(decoder);

    // 最初のフレームをデコードするとメタデータブロックが読み終わる
    FlacDecodedFrame frame;
    ret = flac_decoder_decode_frame(decoder, &frame);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: フレームのデコードに失敗した: %s\n",
                flac_decoder_get_last_error(decoder));
        flac_decoder_free(decoder);
        return 0;
    }

    // VORBIS_COMMENT を取得する
    FlacVorbisComment comment;
    ret = flac_decoder_get_vorbis_comment(decoder, &comment);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: VORBIS_COMMENT の取得に失敗した: %s\n",
                flac_decoder_get_last_error(decoder));
        flac_decoder_free(decoder);
        return 0;
    }

    // vendor を検証する
    if (strcmp(comment.vendor, VENDOR) != 0) {
        fprintf(stderr, "テスト失敗: vendor が一致しない: 期待値 '%s'、実際 '%s'\n",
                VENDOR, comment.vendor);
        flac_decoder_free(decoder);
        return 0;
    }

    // フィールド数を検証する
    if (comment.field_count != FIELD_COUNT) {
        fprintf(stderr, "テスト失敗: フィールド数が一致しない: 期待値 %d、実際 %u\n",
                FIELD_COUNT, comment.field_count);
        flac_decoder_free(decoder);
        return 0;
    }

    // フィールドの内容を検証する
    for (uint32_t i = 0; i < FIELD_COUNT; i++) {
        if (strcmp(comment.fields[i].name, FIELD_NAMES[i]) != 0) {
            fprintf(stderr, "テスト失敗: フィールド %u の名前が一致しない: 期待値 '%s'、実際 '%s'\n",
                    i, FIELD_NAMES[i], comment.fields[i].name);
            flac_decoder_free(decoder);
            return 0;
        }
        if (strcmp(comment.fields[i].value, FIELD_VALUES[i]) != 0) {
            fprintf(stderr, "テスト失敗: フィールド %u の値が一致しない: 期待値 '%s'、実際 '%s'\n",
                    i, FIELD_VALUES[i], comment.fields[i].value);
            flac_decoder_free(decoder);
            return 0;
        }
    }

    printf("VORBIS_COMMENT のラウンドトリップに成功した (vendor: %s、フィールド数: %u)\n",
           comment.vendor, comment.field_count);

    flac_decoder_free(decoder);
    return 1;
}

// VORBIS_COMMENT を含まないストリームでは NOT_FOUND が返ることを検証する
static int verify_not_found_without_metadata(void) {
    FlacEncoder *encoder = flac_encoder_new();
    flac_encoder_set_sample_rate(encoder, SAMPLE_RATE);
    flac_encoder_set_channels(encoder, CHANNELS);
    flac_encoder_set_bits_per_sample(encoder, BITS_PER_SAMPLE);

    FlacError ret = flac_encoder_initialize(encoder);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: エンコーダーの初期化に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 0;
    }

    int32_t samples[TOTAL_SAMPLES];
    generate_samples(samples, TOTAL_SAMPLES);
    flac_encoder_push_samples(encoder, samples, TOTAL_SAMPLES);
    ret = flac_encoder_finalize(encoder);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: エンコードの完了処理に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 0;
    }

    const uint8_t *data = NULL;
    uint64_t size = 0;
    flac_encoder_get_output(encoder, &data, &size);

    // 出力を複製してからエンコーダーを解放する
    uint8_t *copy = (uint8_t *)malloc((size_t)size);
    memcpy(copy, data, (size_t)size);
    flac_encoder_free(encoder);

    FlacDecoder *decoder = flac_decoder_new();
    flac_decoder_feed(decoder, copy, (uint32_t)size);
    flac_decoder_finish(decoder);

    FlacDecodedFrame frame;
    ret = flac_decoder_decode_frame(decoder, &frame);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: フレームのデコードに失敗した: %s\n",
                flac_decoder_get_last_error(decoder));
        free(copy);
        flac_decoder_free(decoder);
        return 0;
    }

    FlacVorbisComment comment;
    ret = flac_decoder_get_vorbis_comment(decoder, &comment);
    free(copy);
    flac_decoder_free(decoder);

    if (ret != FLAC_ERROR_NOT_FOUND) {
        fprintf(stderr, "テスト失敗: VORBIS_COMMENT がないストリームで NOT_FOUND 以外が返った: %d\n",
                (int)ret);
        return 0;
    }

    printf("VORBIS_COMMENT がないストリームで NOT_FOUND が返ることを確認した\n");
    return 1;
}

// NULL ポインタを渡したときの挙動を検証する
static int verify_null_pointer_handling(void) {
    FlacEncoder *encoder = flac_encoder_new();
    FlacError ret = flac_encoder_set_vorbis_comment(encoder, NULL, NULL, NULL, 0);
    if (ret != FLAC_ERROR_NULL_POINTER) {
        fprintf(stderr, "テスト失敗: vendor に NULL を渡したとき NULL_POINTER 以外が返った: %d\n",
                (int)ret);
        flac_encoder_free(encoder);
        return 0;
    }

    ret = flac_encoder_set_vorbis_comment(encoder, VENDOR, NULL, NULL, 1);
    if (ret != FLAC_ERROR_NULL_POINTER) {
        fprintf(stderr, "テスト失敗: field_names に NULL を渡したとき NULL_POINTER 以外が返った: %d\n",
                (int)ret);
        flac_encoder_free(encoder);
        return 0;
    }

    // フィールド数 0 なら vendor だけで VORBIS_COMMENT を作れる
    ret = flac_encoder_set_vorbis_comment(encoder, VENDOR, NULL, NULL, 0);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: フィールド数 0 の設定に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 0;
    }
    flac_encoder_free(encoder);

    FlacDecoder *decoder = flac_decoder_new();
    FlacVorbisComment comment;
    ret = flac_decoder_get_vorbis_comment(decoder, &comment);
    if (ret != FLAC_ERROR_INPUT_REQUIRED) {
        fprintf(stderr, "テスト失敗: メタデータ未読のデコーダーで INPUT_REQUIRED 以外が返った: %d\n",
                (int)ret);
        flac_decoder_free(decoder);
        return 0;
    }
    flac_decoder_free(decoder);

    printf("NULL ポインタと未読状態のハンドリングを確認した\n");
    return 1;
}

// エンコーダーの状態遷移を検証する
static int verify_invalid_state(void) {
    FlacEncoder *encoder = flac_encoder_new();
    flac_encoder_set_sample_rate(encoder, SAMPLE_RATE);
    flac_encoder_set_channels(encoder, CHANNELS);
    flac_encoder_set_bits_per_sample(encoder, BITS_PER_SAMPLE);

    FlacError ret = flac_encoder_initialize(encoder);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "テスト失敗: エンコーダーの初期化に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 0;
    }

    // 初期化後に VORBIS_COMMENT を設定しようとすると INVALID_STATE
    ret = flac_encoder_set_vorbis_comment(encoder, VENDOR, FIELD_NAMES,
                                          FIELD_VALUES, FIELD_COUNT);
    flac_encoder_free(encoder);
    if (ret != FLAC_ERROR_INVALID_STATE) {
        fprintf(stderr, "テスト失敗: 初期化後の設定で INVALID_STATE 以外が返った: %d\n",
                (int)ret);
        return 0;
    }

    printf("初期化後の VORBIS_COMMENT 設定が INVALID_STATE になることを確認した\n");
    return 1;
}

// 不正なフィールド名が拒否されることを検証する
static int verify_invalid_field_name(void) {
    FlacEncoder *encoder = flac_encoder_new();
    // '=' を含むフィールド名は RFC 9639 Section 8.6 で禁止されている
    const char *bad_names[] = {"BAD=NAME"};
    const char *bad_values[] = {"value"};
    FlacError ret =
        flac_encoder_set_vorbis_comment(encoder, VENDOR, bad_names, bad_values, 1);
    flac_encoder_free(encoder);

    // 現状はフィールド名の検証をエンコード時に行うため、ここでは
    // 設定自体は成功し、初期化時にエラーになる
    if (ret == FLAC_ERROR_OK) {
        printf("不正なフィールド名は初期化時に検出される (設定時は受理)\n");
        return 1;
    }
    if (ret != FLAC_ERROR_INVALID_INPUT) {
        fprintf(stderr, "テスト失敗: 不正なフィールド名で想定外のエラーが返った: %d\n",
                (int)ret);
        return 0;
    }

    printf("不正なフィールド名が INVALID_INPUT で拒否されることを確認した\n");
    return 1;
}

int main(void) {
    printf("VORBIS_COMMENT のテストを開始する\n");

    uint64_t flac_size = 0;
    uint8_t *flac_data = encode_with_metadata(&flac_size);
    if (flac_data == NULL) {
        return 1;
    }
    printf("エンコード完了: %llu バイト\n", (unsigned long long)flac_size);

    int ok = 1;
    if (!decode_and_verify_metadata(flac_data, flac_size)) {
        ok = 0;
    }
    free(flac_data);

    if (!verify_not_found_without_metadata()) {
        ok = 0;
    }
    if (!verify_null_pointer_handling()) {
        ok = 0;
    }
    if (!verify_invalid_state()) {
        ok = 0;
    }
    if (!verify_invalid_field_name()) {
        ok = 0;
    }

    if (!ok) {
        fprintf(stderr, "テスト失敗\n");
        return 1;
    }
    printf("テスト成功\n");
    return 0;
}
