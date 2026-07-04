// サイン波を生成して FLAC ファイルにエンコードするサンプル
//
// 使い方: ./encode /path/to/output.flac

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include "flac.h"

// エンコードする音声の形式
#define SAMPLE_RATE 44100
#define CHANNELS 2
#define BITS_PER_SAMPLE 16

// エンコードする長さ (秒)
#define DURATION_SECONDS 2

// 1 回の投入サンプル数 (ストリーミング利用を模したチャンク分割)
#define CHUNK_SAMPLES 4096

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <output.flac>\n", argv[0]);
        return 1;
    }
    const char *output_path = argv[1];

    printf("shiguredo_flac version: %s\n", flac_library_version());

    // 1. FlacEncoder インスタンスを生成して設定する
    FlacEncoder *encoder = flac_encoder_new();
    flac_encoder_set_sample_rate(encoder, SAMPLE_RATE);
    flac_encoder_set_channels(encoder, CHANNELS);
    flac_encoder_set_bits_per_sample(encoder, BITS_PER_SAMPLE);

    // 2. エンコード処理を初期化する
    FlacError ret = flac_encoder_initialize(encoder);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "Failed to initialize encoder: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 1;
    }

    // 3. サイン波 (左 440 Hz / 右 660 Hz) を生成してチャンクごとに投入する
    int32_t chunk[CHUNK_SAMPLES * CHANNELS];
    const uint64_t total_samples = (uint64_t)SAMPLE_RATE * DURATION_SECONDS;
    uint64_t generated = 0;
    while (generated < total_samples) {
        uint32_t n = CHUNK_SAMPLES;
        if (total_samples - generated < n) {
            n = (uint32_t)(total_samples - generated);
        }
        for (uint32_t i = 0; i < n; i++) {
            const double t = (double)(generated + i) / SAMPLE_RATE;
            chunk[i * 2] = (int32_t)(10000.0 * sin(2.0 * M_PI * 440.0 * t));
            chunk[i * 2 + 1] = (int32_t)(10000.0 * sin(2.0 * M_PI * 660.0 * t));
        }
        ret = flac_encoder_push_samples(encoder, chunk, n * CHANNELS);
        if (ret != FLAC_ERROR_OK) {
            fprintf(stderr, "Failed to push samples: %s\n",
                    flac_encoder_get_last_error(encoder));
            flac_encoder_free(encoder);
            return 1;
        }
        generated += n;
    }

    // 4. エンコード処理を完了する
    ret = flac_encoder_finalize(encoder);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "Failed to finalize encoder: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 1;
    }

    // 5. 完成した FLAC ストリームをファイルに書き込む
    const uint8_t *output_data;
    uint64_t output_size;
    ret = flac_encoder_get_output(encoder, &output_data, &output_size);
    if (ret != FLAC_ERROR_OK) {
        fprintf(stderr, "Failed to get output: %s\n",
                flac_encoder_get_last_error(encoder));
        flac_encoder_free(encoder);
        return 1;
    }

    FILE *fp = fopen(output_path, "wb");
    if (fp == NULL) {
        fprintf(stderr, "Failed to open output file: %s\n", output_path);
        flac_encoder_free(encoder);
        return 1;
    }
    if (fwrite(output_data, 1, output_size, fp) != output_size) {
        fprintf(stderr, "Failed to write output file: %s\n", output_path);
        fclose(fp);
        flac_encoder_free(encoder);
        return 1;
    }
    fclose(fp);

    printf("Encoded %llu samples (%d channels, %d bits, %d Hz) into %s (%llu bytes)\n",
           (unsigned long long)total_samples, CHANNELS, BITS_PER_SAMPLE, SAMPLE_RATE,
           output_path, (unsigned long long)output_size);

    // 6. リソース解放
    flac_encoder_free(encoder);
    return 0;
}
