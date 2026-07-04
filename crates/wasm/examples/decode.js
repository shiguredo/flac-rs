/**
 * Node.js で flac-rs の WASM を使って FLAC ファイルをデコードする例
 *
 * # 使用方法
 *
 * ```bash
 * cargo build -p wasm --target wasm32-unknown-unknown --profile release-wasm
 * node crates/wasm/examples/decode.js /path/to/input.flac
 * ```
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// FlacError の値 (include/flac.h の FlacError に対応)
const FLAC_ERROR_OK = 0;
const FLAC_ERROR_INPUT_REQUIRED = 4;
const FLAC_ERROR_NO_MORE_FRAMES = 6;

// 1 回の投入サイズ (バイト)
const CHUNK_SIZE = 64 * 1024;

// FlacDecodedFrame / FlacStreamInfo 構造体の確保サイズ
// (実際のサイズ以上であればよい)
const FRAME_STRUCT_SIZE = 32;
const STREAM_INFO_STRUCT_SIZE = 64;

// wasm ファイルを読み込んで初期化する
const wasmPath = path.join(__dirname, '../../../target/wasm32-unknown-unknown/release-wasm/flac_wasm.wasm');
const wasmBuffer = fs.readFileSync(wasmPath);
const wasmInstance = await WebAssembly.instantiate(wasmBuffer);

const {
    memory,
    flac_library_version,
    flac_decoder_new,
    flac_decoder_free,
    flac_decoder_get_last_error,
    flac_decoder_feed,
    flac_decoder_finish,
    flac_decoder_decode_frame,
    flac_decoder_get_stream_info,
    flac_stream_info_to_json,
    flac_decoded_frame_to_json,
    flac_vec_ptr,
    flac_vec_len,
    flac_vec_free,
    flac_alloc,
    flac_free,
} = wasmInstance.instance.exports;

// NULL 終端の C 文字列を読み取る
function readCString(ptr) {
    const view = new Uint8Array(memory.buffer, ptr);
    let length = 0;
    while (view[length] !== 0) length++;
    return new TextDecoder().decode(view.slice(0, length));
}

// `Vec<u8>` に格納された JSON 文字列を読み取って解放する
function readJSON(vecPtr) {
    const ptr = flac_vec_ptr(vecPtr);
    const len = flac_vec_len(vecPtr);
    const bytes = new Uint8Array(memory.buffer, ptr, len);
    const jsonStr = new TextDecoder().decode(bytes);
    flac_vec_free(vecPtr);
    return JSON.parse(jsonStr);
}

function decodeFlacFile(filePath) {
    if (!fs.existsSync(filePath)) {
        console.error(`Error: File not found: ${filePath}`);
        process.exit(1);
    }

    console.log(`shiguredo_flac version: ${readCString(flac_library_version())}`);
    console.log(`Decoding ${path.basename(filePath)}...`);

    const fileData = fs.readFileSync(filePath);

    // デコーダーを作成する
    const decoderPtr = flac_decoder_new();

    // FLAC データをチャンクごとに投入して終端を通知する
    // (ストリーミングであれば受信のたびに feed とデコードを繰り返せばよい)
    for (let offset = 0; offset < fileData.length; offset += CHUNK_SIZE) {
        const chunk = fileData.subarray(offset, offset + CHUNK_SIZE);
        const chunkPtr = flac_alloc(chunk.length);
        new Uint8Array(memory.buffer, chunkPtr, chunk.length).set(chunk);
        if (flac_decoder_feed(decoderPtr, chunkPtr, chunk.length) !== FLAC_ERROR_OK) {
            const errorMsg = readCString(flac_decoder_get_last_error(decoderPtr));
            console.error(`Error: Failed to feed data: ${errorMsg}`);
            flac_decoder_free(decoderPtr);
            process.exit(1);
        }
        flac_free(chunkPtr, chunk.length);
    }
    flac_decoder_finish(decoderPtr);

    // フレームを順番にデコードする
    const framePtr = flac_alloc(FRAME_STRUCT_SIZE);
    let streamInfoPrinted = false;
    let totalFrames = 0;
    let totalSamples = 0;
    while (true) {
        const ret = flac_decoder_decode_frame(decoderPtr, framePtr);
        if (ret === FLAC_ERROR_NO_MORE_FRAMES) {
            // すべてのフレームをデコードし終えた
            // (この時点で MD5 / 総サンプル数の検証も完了している)
            break;
        }
        if (ret !== FLAC_ERROR_OK) {
            const errorMsg = readCString(flac_decoder_get_last_error(decoderPtr));
            console.error(`Error: Failed to decode frame: ${errorMsg}`);
            flac_decoder_free(decoderPtr);
            process.exit(1);
        }

        // 最初のフレームがデコードできた時点で STREAMINFO は必ず取得できる
        if (!streamInfoPrinted) {
            const streamInfoPtr = flac_alloc(STREAM_INFO_STRUCT_SIZE);
            if (flac_decoder_get_stream_info(decoderPtr, streamInfoPtr) === FLAC_ERROR_OK) {
                const info = readJSON(flac_stream_info_to_json(streamInfoPtr));
                console.log('\nStream info:');
                console.log(`  Sample rate: ${info.sample_rate} Hz`);
                console.log(`  Channels: ${info.channels}`);
                console.log(`  Bits per sample: ${info.bits_per_sample}`);
                console.log(`  Total samples: ${info.total_samples}`);
                console.log(`  Block size: ${info.min_block_size}-${info.max_block_size}`);
                console.log(`  MD5: ${info.md5}`);
                console.log();
                streamInfoPrinted = true;
            }
            flac_free(streamInfoPtr, STREAM_INFO_STRUCT_SIZE);
        }

        const frame = readJSON(flac_decoded_frame_to_json(framePtr));

        // サンプル列は wasm メモリから直接読み出せる
        // (このビューは次の wasm 関数の呼び出しまでのみ有効)
        const samples = new Int32Array(memory.buffer, frame.samples_offset, frame.sample_count);

        if (totalFrames < 5) {
            console.log(`  Frame ${totalFrames + 1}: ${frame.sample_count / frame.channels} samples, first sample number ${frame.first_sample_number}, first values [${samples[0]}, ${samples[1]}]`);
        } else if (totalFrames === 5) {
            console.log('  ... (showing first 5 frames)');
        }

        totalFrames++;
        totalSamples += frame.sample_count / frame.channels;
    }
    flac_free(framePtr, FRAME_STRUCT_SIZE);

    console.log(`\nDecoded ${totalFrames} frames (${totalSamples} samples), stream integrity verified`);

    // リソースを解放する
    flac_decoder_free(decoderPtr);
}

// メイン処理
const args = process.argv.slice(2);
if (args.length === 0) {
    console.error('Usage: node decode.js <flac_file>');
    process.exit(1);
}

decodeFlacFile(args[0]);
