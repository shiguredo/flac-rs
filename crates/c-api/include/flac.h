#ifndef SHIGUREDO_FLAC_H
#define SHIGUREDO_FLAC_H

/* Generated with cbindgen:0.29.4 */

#include <stdbool.h>
#include <stdint.h>

/**
 * 発生する可能性のあるエラーの種類を表現する列挙型
 */
typedef enum FlacError {
  /**
   * エラーが発生しなかったことを示す
   */
  FLAC_ERROR_OK = 0,
  /**
   * 入力引数ないしパラメーターが無効である
   */
  FLAC_ERROR_INVALID_INPUT,
  /**
   * 入力データが破損しているか無効な形式である
   */
  FLAC_ERROR_INVALID_DATA,
  /**
   * 操作に対する内部状態が無効である
   */
  FLAC_ERROR_INVALID_STATE,
  /**
   * 入力データの読み込みが必要である
   */
  FLAC_ERROR_INPUT_REQUIRED,
  /**
   * NULL ポインタが渡された
   */
  FLAC_ERROR_NULL_POINTER,
  /**
   * これ以上デコードするフレームが存在しない
   */
  FLAC_ERROR_NO_MORE_FRAMES,
} FlacError;

/**
 * FLAC ストリームのデコード処理を行うための構造体
 *
 * # 関連関数
 *
 * この構造体は、以下の関数を通して操作する必要がある:
 * - `flac_decoder_new()`: `FlacDecoder` インスタンスを生成する
 * - `flac_decoder_free()`: リソースを解放する
 * - `flac_decoder_feed()`: 入力データを投入する
 * - `flac_decoder_finish()`: 入力の終端を通知する
 * - `flac_decoder_decode_frame()`: フレームをひとつデコードする
 * - `flac_decoder_get_stream_info()`: STREAMINFO を取得する
 * - `flac_decoder_get_last_error()`: 最後に発生したエラーのメッセージを取得する
 *
 * # 使用例
 *
 * ```c
 * #include <stdio.h>
 * #include <stdlib.h>
 * #include "flac.h"
 *
 * int main(void) {
 *     // 1. FlacDecoder インスタンスを生成
 *     FlacDecoder *decoder = flac_decoder_new();
 *
 *     // 2. FLAC データを投入 (ストリーミングであれば受信のたびに呼ぶ)
 *     const uint8_t *flac_data = ...;
 *     uint32_t flac_size = ...;
 *     flac_decoder_feed(decoder, flac_data, flac_size);
 *
 *     // 3. 入力の終端を通知 (これにより MD5 検証などの完全性チェックが行われる)
 *     flac_decoder_finish(decoder);
 *
 *     // 4. フレームを順番にデコード
 *     FlacDecodedFrame frame;
 *     FlacError ret;
 *     while ((ret = flac_decoder_decode_frame(decoder, &frame)) == FLAC_ERROR_OK) {
 *         for (uint32_t i = 0; i < frame.sample_count; i++) {
 *             // frame.samples[i] を処理する...
 *         }
 *     }
 *     if (ret != FLAC_ERROR_NO_MORE_FRAMES) {
 *         fprintf(stderr, "Failed to decode frame: %s\n", flac_decoder_get_last_error(decoder));
 *         flac_decoder_free(decoder);
 *         return 1;
 *     }
 *
 *     // 5. リソース解放
 *     flac_decoder_free(decoder);
 *     return 0;
 * }
 * ```
 */
typedef struct FlacDecoder FlacDecoder;

/**
 * PCM サンプルの FLAC ストリームへのエンコード処理を行うための構造体
 *
 * STREAMINFO の合計サンプル数・MD5・フレームサイズ統計はエンコード完了時に
 * 確定するため、出力は `flac_encoder_finalize()` の後にまとめて取得する
 *
 * # 関連関数
 *
 * この構造体は、以下の関数を通して操作する必要がある:
 * - `flac_encoder_new()`: `FlacEncoder` インスタンスを生成する
 * - `flac_encoder_free()`: リソースを解放する
 * - `flac_encoder_set_sample_rate()` などの設定関数: エンコード設定を変更する
 * - `flac_encoder_initialize()`: エンコード処理を初期化する
 * - `flac_encoder_push_samples()`: サンプルを投入する
 * - `flac_encoder_finalize()`: エンコード処理を完了する
 * - `flac_encoder_get_output()`: 完成した FLAC ストリームを取得する
 * - `flac_encoder_get_last_error()`: 最後に発生したエラーのメッセージを取得する
 *
 * # 使用例
 *
 * ```c
 * #include <stdio.h>
 * #include <stdlib.h>
 * #include "flac.h"
 *
 * int main(void) {
 *     // 1. FlacEncoder インスタンスを生成
 *     FlacEncoder *encoder = flac_encoder_new();
 *
 *     // 2. エンコード設定 (初期化前に行う必要がある)
 *     flac_encoder_set_sample_rate(encoder, 44100);
 *     flac_encoder_set_channels(encoder, 2);
 *     flac_encoder_set_bits_per_sample(encoder, 16);
 *
 *     // 3. エンコード処理を初期化
 *     FlacError ret = flac_encoder_initialize(encoder);
 *     if (ret != FLAC_ERROR_OK) {
 *         fprintf(stderr, "Failed to initialize encoder: %s\n",
 *                 flac_encoder_get_last_error(encoder));
 *         flac_encoder_free(encoder);
 *         return 1;
 *     }
 *
 *     // 4. チャンネルインターリーブ済み (L, R, L, R, ...) のサンプルを投入
 *     int32_t samples[] = {100, -100, 200, -200};
 *     ret = flac_encoder_push_samples(encoder, samples, 4);
 *     if (ret != FLAC_ERROR_OK) {
 *         fprintf(stderr, "Failed to push samples: %s\n",
 *                 flac_encoder_get_last_error(encoder));
 *         flac_encoder_free(encoder);
 *         return 1;
 *     }
 *
 *     // 5. エンコード処理を完了
 *     ret = flac_encoder_finalize(encoder);
 *     if (ret != FLAC_ERROR_OK) {
 *         fprintf(stderr, "Failed to finalize encoder: %s\n",
 *                 flac_encoder_get_last_error(encoder));
 *         flac_encoder_free(encoder);
 *         return 1;
 *     }
 *
 *     // 6. 完成した FLAC ストリームを取得してファイルに書き込む
 *     const uint8_t *output_data;
 *     uint64_t output_size;
 *     flac_encoder_get_output(encoder, &output_data, &output_size);
 *     FILE *fp = fopen("output.flac", "wb");
 *     fwrite(output_data, 1, output_size, fp);
 *     fclose(fp);
 *
 *     // 7. リソース解放
 *     flac_encoder_free(encoder);
 *     return 0;
 * }
 * ```
 */
typedef struct FlacEncoder FlacEncoder;

/**
 * デコードした 1 フレーム分の音声を表す構造体
 *
 * `samples` が参照するバッファは `FlacDecoder` が所有しており、
 * 同じデコーダーに対して次の `flac_decoder_decode_frame()` を呼び出すか
 * `flac_decoder_free()` を呼び出すと無効になる
 */
typedef struct FlacDecodedFrame {
  /**
   * チャンネルインターリーブ済み (L, R, L, R, ...) のサンプル列へのポインタ
   */
  const int32_t *samples;
  /**
   * `samples` の要素数 (ブロックサイズ x チャンネル数)
   */
  uint32_t sample_count;
  /**
   * 実効サンプルレート (Hz)
   */
  uint32_t sample_rate;
  /**
   * このフレームの最初のインターチャンネルサンプル番号
   */
  uint64_t first_sample_number;
  /**
   * チャンネル数
   */
  uint8_t channels;
  /**
   * 実効ビット深度
   */
  uint8_t bits_per_sample;
} FlacDecodedFrame;

/**
 * FLAC ストリームの STREAMINFO メタデータを表す構造体 (RFC 9639 Section 8.2)
 */
typedef struct FlacStreamInfo {
  /**
   * ストリーム中の最小ブロックサイズ (サンプル数、最終ブロックを除く)。16-65535
   */
  uint16_t min_block_size;
  /**
   * ストリーム中の最大ブロックサイズ (サンプル数)。16-65535
   */
  uint16_t max_block_size;
  /**
   * 最小フレームサイズ (バイト)。0 は不明を表す
   */
  uint32_t min_frame_size;
  /**
   * 最大フレームサイズ (バイト)。0 は不明を表す
   */
  uint32_t max_frame_size;
  /**
   * サンプルレート (Hz)。1-1048575
   */
  uint32_t sample_rate;
  /**
   * チャンネル数 (1-8)
   */
  uint8_t channels;
  /**
   * サンプルあたりのビット数 (4-32)
   */
  uint8_t bits_per_sample;
  /**
   * 総インターチャンネルサンプル数。0 は不明を表す
   */
  uint64_t total_samples;
  /**
   * エンコード前オーディオデータの MD5 チェックサム。全て 0 は不明を表す
   */
  uint8_t md5[16];
} FlacStreamInfo;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * ライブラリのバージョンを取得する
 *
 * # 戻り値
 *
 * バージョン文字列へのポインタ（NULL 終端）
 */
const char *flac_library_version(void);

/**
 * 新しい `FlacDecoder` インスタンスを作成して、それへのポインタを返す
 *
 * 返されたポインタは、使用後に `flac_decoder_free()` で破棄する必要がある
 *
 * # 戻り値
 *
 * 新しく作成された `FlacDecoder` インスタンスへのポインタ
 * （現在の実装では NULL ポインタが返されることはない）
 */
struct FlacDecoder *flac_decoder_new(void);

/**
 * `FlacDecoder` インスタンスを破棄して、割り当てられたリソースを解放する
 *
 * # 引数
 *
 * - `decoder`: 破棄する `FlacDecoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、この関数は何もしない
 *
 * # 注意
 *
 * この関数の呼び出し後は、`flac_decoder_decode_frame()` で取得した
 * `FlacDecodedFrame::samples` のポインタも無効になる
 */
void flac_decoder_free(struct FlacDecoder *decoder);

/**
 * `FlacDecoder` で最後に発生したエラーのメッセージを取得する
 *
 * # 引数
 *
 * - `decoder`: `FlacDecoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、NULL 終端の空文字列へのポインタを返す
 *
 * # 戻り値
 *
 * - メッセージが存在する場合: NULL 終端のエラーメッセージへのポインタ
 * - メッセージが存在しない場合: NULL 終端の空文字列へのポインタ
 */
const char *flac_decoder_get_last_error(const struct FlacDecoder *decoder);

/**
 * FLAC ストリームの入力データを投入する
 *
 * ストリーム全体を一括で渡しても、受信のたびに分割して渡してもよい
 * (分割の境界はフレーム境界と一致していなくてもよい)
 *
 * # 引数
 *
 * - `decoder`: `FlacDecoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `data`: 入力データへのポインタ
 *   - `size` が 0 より大きいのに NULL が渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `size`: 入力データのサイズ（バイト単位）
 *   - 0 を指定した場合は何もしない
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常にデータが投入された
 * - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
 * - `FLAC_ERROR_INVALID_STATE`: `flac_decoder_finish()` の呼び出し後に呼ばれた
 */
enum FlacError flac_decoder_feed(struct FlacDecoder *decoder,
                                 const uint8_t *data,
                                 uint32_t size);

/**
 * FLAC ストリームの入力の終端を通知する
 *
 * 終端通知により、途中で切れたストリームの検出と MD5 チェックサムの検証が
 * 行われるようになる (RFC 9639 Section 8.2)
 *
 * この関数の呼び出し後に `flac_decoder_decode_frame()` が残りのフレームを
 * 返し終えると、`FLAC_ERROR_NO_MORE_FRAMES` が返されるようになる
 *
 * # 引数
 *
 * - `decoder`: `FlacDecoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に終端が通知された
 * - `FLAC_ERROR_NULL_POINTER`: `decoder` が NULL である
 */
enum FlacError flac_decoder_finish(struct FlacDecoder *decoder);

/**
 * FLAC ストリームからフレームをひとつデコードする
 *
 * # 引数
 *
 * - `decoder`: `FlacDecoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `out_frame`: デコード結果を受け取る `FlacDecodedFrame` 構造体へのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常にフレームがデコードされた
 * - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
 * - `FLAC_ERROR_INPUT_REQUIRED`: フレームのデコードに必要な入力データが不足している
 *   - `flac_decoder_feed()` で追加のデータを投入するか、
 *     入力の終端であれば `flac_decoder_finish()` を呼び出す必要がある
 * - `FLAC_ERROR_NO_MORE_FRAMES`: すべてのフレームをデコードし終えた
 *   - この時点でストリームの完全性 (MD5 / 総サンプル数) の検証も完了している
 * - `FLAC_ERROR_INVALID_DATA`: 入力データが FLAC として不正である
 *
 * # 注意
 *
 * `out_frame->samples` が参照するバッファは `FlacDecoder` が所有しており、
 * 同じデコーダーに対して次の `flac_decoder_decode_frame()` を呼び出すか
 * `flac_decoder_free()` を呼び出すと無効になる
 */
enum FlacError flac_decoder_decode_frame(struct FlacDecoder *decoder,
                                         struct FlacDecodedFrame *out_frame);

/**
 * FLAC ストリームの STREAMINFO メタデータを取得する
 *
 * STREAMINFO はストリームの先頭に位置するため、
 * 最初のフレームがデコードできる時点では必ず取得できる
 *
 * # 引数
 *
 * - `decoder`: `FlacDecoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `out_stream_info`: STREAMINFO を受け取る `FlacStreamInfo` 構造体へのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に STREAMINFO が取得された
 * - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
 * - `FLAC_ERROR_INPUT_REQUIRED`: STREAMINFO のデコードに必要な入力データが不足している
 */
enum FlacError flac_decoder_get_stream_info(struct FlacDecoder *decoder,
                                            struct FlacStreamInfo *out_stream_info);

/**
 * 新しい `FlacEncoder` インスタンスを作成して、それへのポインタを返す
 *
 * 返されたポインタは、使用後に `flac_encoder_free()` で破棄する必要がある
 *
 * エンコード設定は以下のデフォルト値で初期化される:
 * - サンプルレート: 44100 Hz
 * - チャンネル数: 2
 * - ビット深度: 16
 * - ブロックサイズ: 4096
 * - LPC の最大次数: 8
 * - Rice パーティションの最大オーダー: 4
 * - ステレオデコリレーション: 有効
 *
 * # 戻り値
 *
 * 新しく作成された `FlacEncoder` インスタンスへのポインタ
 * （現在の実装では NULL ポインタが返されることはない）
 */
struct FlacEncoder *flac_encoder_new(void);

/**
 * `FlacEncoder` インスタンスを破棄して、割り当てられたリソースを解放する
 *
 * # 引数
 *
 * - `encoder`: 破棄する `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、この関数は何もしない
 *
 * # 注意
 *
 * この関数の呼び出し後は、`flac_encoder_get_output()` で取得した
 * 出力データへのポインタも無効になる
 */
void flac_encoder_free(struct FlacEncoder *encoder);

/**
 * `FlacEncoder` で最後に発生したエラーのメッセージを取得する
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、NULL 終端の空文字列へのポインタを返す
 *
 * # 戻り値
 *
 * - メッセージが存在する場合: NULL 終端のエラーメッセージへのポインタ
 * - メッセージが存在しない場合: NULL 終端の空文字列へのポインタ
 */
const char *flac_encoder_get_last_error(const struct FlacEncoder *encoder);

/**
 * エンコードするストリームのサンプルレート (Hz) を設定する
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `sample_rate`: サンプルレート (Hz)。1-1048575 (RFC 9639 Section 8.2)
 *   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に設定された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 */
enum FlacError flac_encoder_set_sample_rate(struct FlacEncoder *encoder, uint32_t sample_rate);

/**
 * エンコードするストリームのチャンネル数を設定する
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `channels`: チャンネル数 (1-8) (RFC 9639 Section 8.2)
 *   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に設定された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 */
enum FlacError flac_encoder_set_channels(struct FlacEncoder *encoder, uint8_t channels);

/**
 * エンコードするストリームのサンプルあたりのビット数を設定する
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `bits_per_sample`: サンプルあたりのビット数 (4-32) (RFC 9639 Section 8.2)
 *   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に設定された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 */
enum FlacError flac_encoder_set_bits_per_sample(struct FlacEncoder *encoder,
                                                uint8_t bits_per_sample);

/**
 * エンコードのブロックサイズ (インターチャンネルサンプル数) を設定する
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `block_size`: ブロックサイズ (16-65535) (RFC 9639 Section 8.2)
 *   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に設定された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 */
enum FlacError flac_encoder_set_block_size(struct FlacEncoder *encoder,
                                           uint16_t block_size);

/**
 * LPC の最大次数を設定する
 *
 * 大きいほど圧縮率が上がる可能性があるが、エンコードは遅くなる
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `max_lpc_order`: LPC の最大次数 (0-32)。0 なら固定予測のみ使う (RFC 9639 Section 9.2.6)
 *   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に設定された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 */
enum FlacError flac_encoder_set_max_lpc_order(struct FlacEncoder *encoder,
                                              uint8_t max_lpc_order);

/**
 * Rice パーティションの最大オーダーを設定する
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `max_partition_order`: Rice パーティションの最大オーダー (0-15) (RFC 9639 Section 9.2.7)
 *   - 範囲外の値は `flac_encoder_initialize()` 呼び出し時にエラーになる
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に設定された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 */
enum FlacError flac_encoder_set_max_partition_order(struct FlacEncoder *encoder,
                                                    uint8_t max_partition_order);

/**
 * ステレオデコリレーション (mid-side 等) を試すかどうかを設定する
 *
 * 2 チャンネルのストリームでのみ効果がある
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `stereo_decorrelation`: ステレオデコリレーションを試すかどうか
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に設定された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 */
enum FlacError flac_encoder_set_stereo_decorrelation(struct FlacEncoder *encoder,
                                                     bool stereo_decorrelation);

/**
 * FLAC ストリームのエンコード処理を初期化する
 *
 * この関数は、それまでに設定されたエンコード設定を検証し、
 * エンコード処理を開始するための準備を行う
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に初期化された
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが既に初期化済みである
 * - `FLAC_ERROR_INVALID_INPUT`: エンコード設定が不正である
 *
 * エラーが発生した場合は、`flac_encoder_get_last_error()` でエラーメッセージを取得できる
 *
 * # オプション指定
 *
 * 以下の設定関数の呼び出しは `flac_encoder_initialize()` の前に行う必要がある:
 * - `flac_encoder_set_sample_rate()`
 * - `flac_encoder_set_channels()`
 * - `flac_encoder_set_bits_per_sample()`
 * - `flac_encoder_set_block_size()`
 * - `flac_encoder_set_max_lpc_order()`
 * - `flac_encoder_set_max_partition_order()`
 * - `flac_encoder_set_stereo_decorrelation()`
 */
enum FlacError flac_encoder_initialize(struct FlacEncoder *encoder);

/**
 * チャンネルインターリーブ済みの PCM サンプルを投入する
 *
 * サンプル数はチャンネル数の倍数でなければならない。ブロックサイズ分の
 * サンプルが揃うたびに内部でフレームにエンコードされる
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `samples`: チャンネルインターリーブ済み (L, R, L, R, ...) のサンプル列へのポインタ
 *   - 各サンプル値はビット深度で表現できる範囲に収まっている必要がある
 *   - `sample_count` が 0 より大きいのに NULL が渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `sample_count`: `samples` の要素数
 *   - チャンネル数の倍数でなければならない
 *   - 0 を指定した場合は何もしない
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常にサンプルが投入された
 * - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが初期化されていないか、既にファイナライズ済み
 * - `FLAC_ERROR_INVALID_INPUT`: サンプル数がチャンネル数の倍数でないか、サンプル値が範囲外
 *
 * エラーが発生した場合は、`flac_encoder_get_last_error()` でエラーメッセージを取得できる
 */
enum FlacError flac_encoder_push_samples(struct FlacEncoder *encoder,
                                         const int32_t *samples,
                                         uint32_t sample_count);

/**
 * FLAC ストリームのエンコード処理を完了する
 *
 * この関数は、残りのサンプルを最終フレームとしてエンコードし、
 * STREAMINFO (合計サンプル数・MD5・フレームサイズ統計) を確定して
 * 完全な FLAC ストリームを構築する
 *
 * エンコード処理が完了すると、`flac_encoder_get_output()` で
 * 完成した FLAC ストリームが取得できるようになる
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常にエンコード処理が完了した
 * - `FLAC_ERROR_NULL_POINTER`: `encoder` が NULL である
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーが初期化されていないか、既にファイナライズ済み
 * - `FLAC_ERROR_INVALID_INPUT`: 最終フレームのエンコードに失敗した
 *
 * エラーが発生した場合は、`flac_encoder_get_last_error()` でエラーメッセージを取得できる
 */
enum FlacError flac_encoder_finalize(struct FlacEncoder *encoder);

/**
 * 完成した FLAC ストリームを取得する
 *
 * この関数は `flac_encoder_finalize()` の呼び出し後にのみ使用できる
 *
 * # 引数
 *
 * - `encoder`: `FlacEncoder` インスタンスへのポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 * - `out_output_data`: 出力データのバッファへのポインタを受け取るポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 *   - 注意: このポインタの参照先は `flac_encoder_free()` を呼び出すと無効になる
 * - `out_output_size`: 出力データのサイズ（バイト単位）を受け取るポインタ
 *   - NULL ポインタが渡された場合、`FLAC_ERROR_NULL_POINTER` が返される
 *
 * # 戻り値
 *
 * - `FLAC_ERROR_OK`: 正常に出力データが取得された
 * - `FLAC_ERROR_NULL_POINTER`: 引数として NULL ポインタが渡された
 * - `FLAC_ERROR_INVALID_STATE`: エンコーダーがまだファイナライズされていない
 */
enum FlacError flac_encoder_get_output(struct FlacEncoder *encoder,
                                       const uint8_t **out_output_data,
                                       uint64_t *out_output_size);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* SHIGUREDO_FLAC_H */
