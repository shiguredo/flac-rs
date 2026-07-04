// FLAC ファイルをデコードして情報を表示するサンプル
//
// ファイル全体を一括で読み込まず、チャンクごとに投入しながらデコードする
// (Sans I/O のストリーミング利用例)
//
// 使い方: ./decode /path/to/input.flac

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include "flac.h"

// 1 回の読み込みサイズ (バイト)
#define CHUNK_SIZE (64 * 1024)

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <input.flac>\n", argv[0]);
        return 1;
    }
    const char *input_path = argv[1];

    printf("shiguredo_flac version: %s\n", flac_library_version());

    FILE *fp = fopen(input_path, "rb");
    if (fp == NULL) {
        fprintf(stderr, "Failed to open input file: %s\n", input_path);
        return 1;
    }

    // 1. FlacDecoder インスタンスを生成する
    FlacDecoder *decoder = flac_decoder_new();

    // 2. フレームをデコードし、入力が足りなければファイルから読み足す
    uint8_t chunk[CHUNK_SIZE];
    int stream_info_printed = 0;
    uint64_t total_frames = 0;
    uint64_t total_samples = 0;
    while (1) {
        FlacDecodedFrame frame;
        FlacError ret = flac_decoder_decode_frame(decoder, &frame);
        if (ret == FLAC_ERROR_OK) {
            // 最初のフレームがデコードできた時点で STREAMINFO は必ず取得できる
            if (!stream_info_printed) {
                FlacStreamInfo info;
                if (flac_decoder_get_stream_info(decoder, &info) == FLAC_ERROR_OK) {
                    printf("Stream info:\n");
                    printf("  Sample rate: %u Hz\n", info.sample_rate);
                    printf("  Channels: %u\n", info.channels);
                    printf("  Bits per sample: %u\n", info.bits_per_sample);
                    printf("  Total samples: %llu\n",
                           (unsigned long long)info.total_samples);
                    printf("  Block size: %u-%u\n", info.min_block_size,
                           info.max_block_size);
                    stream_info_printed = 1;
                }
            }
            total_frames++;
            total_samples += frame.sample_count / frame.channels;
            continue;
        }
        if (ret == FLAC_ERROR_INPUT_REQUIRED) {
            // 入力データが不足しているのでファイルから読み足す
            size_t n = fread(chunk, 1, sizeof(chunk), fp);
            if (n > 0) {
                flac_decoder_feed(decoder, chunk, (uint32_t)n);
            } else {
                // ファイル末尾に達したので入力の終端を通知する
                flac_decoder_finish(decoder);
            }
            continue;
        }
        if (ret == FLAC_ERROR_NO_MORE_FRAMES) {
            // すべてのフレームをデコードし終えた
            // (この時点で MD5 / 総サンプル数の検証も完了している)
            break;
        }
        fprintf(stderr, "Failed to decode frame: %s\n",
                flac_decoder_get_last_error(decoder));
        flac_decoder_free(decoder);
        fclose(fp);
        return 1;
    }

    printf("Decoded %llu frames (%llu samples), stream integrity verified\n",
           (unsigned long long)total_frames, (unsigned long long)total_samples);

    // 3. リソース解放
    flac_decoder_free(decoder);
    fclose(fp);
    return 0;
}
