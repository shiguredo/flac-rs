// メモリ内バッファを使用した FLAC のエンコード / デコードテスト
//
// 以下の処理を実行する:
// 1. 決定的な擬似信号を生成して FLAC ストリームにエンコード
// 2. エンコードした FLAC ストリームをデコード
// 3. 元のサンプルとデコードされたサンプルが完全一致することを確認 (ロスレス性)

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "flac.h"

// テストする音声の形式
#define SAMPLE_RATE 48000
#define CHANNELS 2
#define BITS_PER_SAMPLE 16

// ブロックサイズ非整数倍の総サンプル数 (最終フレームの端数処理も検証する)
#define TOTAL_SAMPLES 10000
#define BLOCK_SIZE 4096

int main(void) {
    printf("FLAC エンコード / デコードのラウンドトリップテストを開始する\n");

    // 元のサンプル列を決定的な擬似乱数 (LCG) と三角波の混合で生成する
    // (浮動小数点を使わずに、それなりに圧縮しづらい信号を作る)
    static int32_t original[TOTAL_SAMPLES * CHANNELS];
    uint32_t lcg = 12345;
    for (int i = 0; i < TOTAL_SAMPLES; i++) {
        // 三角波 (周期 200 サンプル)
        int32_t tri = (i % 200) - 100;
        // 擬似乱数ノイズ (-128 から 127)
        lcg = lcg * 1664525 + 1013904223;
        int32_t noise = (int32_t)(lcg >> 24) - 128;
        original[i * 2] = tri * 50 + noise;
        original[i * 2 + 1] = -tri * 30 + noise;
    }

    // ===== エンコード処理 =====
    printf("エンコードを開始する\n");

    FlacEncoder *encoder = flac_encoder_new();
    if (encoder == NULL) {
        fprintf(stderr, "エンコーダーの生成に失敗した\n");
        return 1;
    }

    // 設定してから初期化する
    if (flac_encoder_set_sample_rate(encoder, SAMPLE_RATE) != FLAC_ERROR_OK ||
        flac_encoder_set_channels(encoder, CHANNELS) != FLAC_ERROR_OK ||
        flac_encoder_set_bits_per_sample(encoder, BITS_PER_SAMPLE) != FLAC_ERROR_OK ||
        flac_encoder_set_block_size(encoder, BLOCK_SIZE) != FLAC_ERROR_OK) {
        fprintf(stderr, "エンコーダーの設定に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 1;
    }

    // 初期化前のサンプル投入は INVALID_STATE になることを確認する
    if (flac_encoder_push_samples(encoder, original, CHANNELS) !=
        FLAC_ERROR_INVALID_STATE) {
        fprintf(stderr, "初期化前のサンプル投入がエラーにならなかった\n");
        flac_encoder_free(encoder);
        return 1;
    }

    if (flac_encoder_initialize(encoder) != FLAC_ERROR_OK) {
        fprintf(stderr, "エンコーダーの初期化に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 1;
    }

    // 初期化後の設定変更は INVALID_STATE になることを確認する
    if (flac_encoder_set_sample_rate(encoder, 96000) != FLAC_ERROR_INVALID_STATE) {
        fprintf(stderr, "初期化後の設定変更がエラーにならなかった\n");
        flac_encoder_free(encoder);
        return 1;
    }

    // サンプルを複数回に分けて投入する (ブロック境界とずれた分割)
    const uint32_t total_values = TOTAL_SAMPLES * CHANNELS;
    const uint32_t chunk_values = 3000 * CHANNELS;
    for (uint32_t offset = 0; offset < total_values; offset += chunk_values) {
        uint32_t n = chunk_values;
        if (total_values - offset < n) {
            n = total_values - offset;
        }
        if (flac_encoder_push_samples(encoder, original + offset, n) != FLAC_ERROR_OK) {
            fprintf(stderr, "サンプルの投入に失敗した: %s\n",
                    flac_encoder_get_last_error(encoder));
            flac_encoder_free(encoder);
            return 1;
        }
    }

    if (flac_encoder_finalize(encoder) != FLAC_ERROR_OK) {
        fprintf(stderr, "エンコーダーのファイナライズに失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 1;
    }

    const uint8_t *flac_data;
    uint64_t flac_size;
    if (flac_encoder_get_output(encoder, &flac_data, &flac_size) != FLAC_ERROR_OK) {
        fprintf(stderr, "エンコード結果の取得に失敗した: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 1;
    }
    printf("エンコード完了: %llu バイト\n", (unsigned long long)flac_size);

    // ===== デコード処理 =====
    printf("デコードを開始する\n");

    FlacDecoder *decoder = flac_decoder_new();
    if (decoder == NULL) {
        fprintf(stderr, "デコーダーの生成に失敗した\n");
        flac_encoder_free(encoder);
        return 1;
    }

    // エンコード結果を一括で投入して終端を通知する
    if (flac_decoder_feed(decoder, flac_data, (uint32_t)flac_size) != FLAC_ERROR_OK) {
        fprintf(stderr, "デコーダーへのデータ投入に失敗した: %s\n",
                flac_decoder_get_last_error(decoder));
        goto fail;
    }
    if (flac_decoder_finish(decoder) != FLAC_ERROR_OK) {
        fprintf(stderr, "デコーダーへの終端通知に失敗した\n");
        goto fail;
    }

    // フレームを順番にデコードして元のサンプルと比較する
    uint64_t decoded_values = 0;
    while (1) {
        FlacDecodedFrame frame;
        FlacError ret = flac_decoder_decode_frame(decoder, &frame);
        if (ret == FLAC_ERROR_NO_MORE_FRAMES) {
            break;
        }
        if (ret != FLAC_ERROR_OK) {
            fprintf(stderr, "フレームのデコードに失敗した: %s\n",
                    flac_decoder_get_last_error(decoder));
            goto fail;
        }
        if (frame.channels != CHANNELS ||
            frame.sample_rate != SAMPLE_RATE ||
            frame.bits_per_sample != BITS_PER_SAMPLE) {
            fprintf(stderr, "フレームの形式が一致しない\n");
            goto fail;
        }
        if (decoded_values + frame.sample_count > total_values) {
            fprintf(stderr, "デコードされたサンプル数が多すぎる\n");
            goto fail;
        }
        // ロスレス性: 元のサンプルと完全一致しなければならない
        if (memcmp(original + decoded_values, frame.samples,
                   frame.sample_count * sizeof(int32_t)) != 0) {
            fprintf(stderr, "デコード結果が元のサンプルと一致しない\n");
            goto fail;
        }
        decoded_values += frame.sample_count;
    }

    if (decoded_values != total_values) {
        fprintf(stderr, "デコードされたサンプル数が一致しない: %llu != %u\n",
                (unsigned long long)decoded_values, total_values);
        goto fail;
    }

    // STREAMINFO の内容を確認する
    FlacStreamInfo info;
    if (flac_decoder_get_stream_info(decoder, &info) != FLAC_ERROR_OK) {
        fprintf(stderr, "STREAMINFO の取得に失敗した: %s\n",
                flac_decoder_get_last_error(decoder));
        goto fail;
    }
    if (info.sample_rate != SAMPLE_RATE || info.channels != CHANNELS ||
        info.bits_per_sample != BITS_PER_SAMPLE ||
        info.total_samples != TOTAL_SAMPLES) {
        fprintf(stderr, "STREAMINFO の内容が一致しない\n");
        goto fail;
    }

    printf("デコード完了: %llu サンプルが元のサンプルと完全一致した\n",
           (unsigned long long)decoded_values / CHANNELS);
    printf("テスト成功\n");

    flac_decoder_free(decoder);
    flac_encoder_free(encoder);
    return 0;

fail:
    flac_decoder_free(decoder);
    flac_encoder_free(encoder);
    return 1;
}
