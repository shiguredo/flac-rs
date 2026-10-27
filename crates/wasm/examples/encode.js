/**
 * Node.js で flac-rs の WASM を使ってサイン波を FLAC ファイルにエンコードする例
 *
 * # 使用方法
 *
 * ```bash
 * cargo build -p wasm --target wasm32-unknown-unknown --profile release-wasm
 * node crates/wasm/examples/encode.js /path/to/output.flac
 * ```
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// FlacError の値 (include/flac.h の FlacError に対応)
const FLAC_ERROR_OK = 0;

// エンコードする音声の形式
const SAMPLE_RATE = 44100;
const CHANNELS = 2;
const BITS_PER_SAMPLE = 16;

// エンコードする長さ (秒)
const DURATION_SECONDS = 2;

// 1 回の投入サンプル数 (ストリーミング利用を模したチャンク分割)
const CHUNK_SAMPLES = 4096;

// wasm ファイルを読み込んで初期化する
const wasmPath = path.join(__dirname, '../../../target/wasm32-unknown-unknown/release-wasm/flac_wasm.wasm');
const wasmBuffer = fs.readFileSync(wasmPath);
const wasmInstance = await WebAssembly.instantiate(wasmBuffer);

const {
    memory,
    flac_library_version,
    flac_encoder_new,
    flac_encoder_free,
    flac_encoder_get_last_error,
    flac_encoder_set_sample_rate,
    flac_encoder_set_channels,
    flac_encoder_set_bits_per_sample,
    flac_encoder_set_vorbis_comment,
    flac_encoder_initialize,
    flac_encoder_push_samples,
    flac_encoder_finalize,
    flac_encoder_get_output,
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

// JavaScript の文字列を wasm メモリに NUL 終端付きで書き込む
//
// 解放できるように `{ ptr, size }` を返す
function writeCString(str) {
    const bytes = new TextEncoder().encode(str);
    const size = bytes.length + 1;
    const ptr = flac_alloc(size);
    new Uint8Array(memory.buffer, ptr, size).set(bytes);
    new Uint8Array(memory.buffer, ptr + bytes.length, 1)[0] = 0;
    return { ptr, size };
}

function encodeFlacFile(filePath) {
    console.log(`shiguredo_flac version: ${readCString(flac_library_version())}`);

    // エンコーダーを作成して設定する
    const encoderPtr = flac_encoder_new();
    flac_encoder_set_sample_rate(encoderPtr, SAMPLE_RATE);
    flac_encoder_set_channels(encoderPtr, CHANNELS);
    flac_encoder_set_bits_per_sample(encoderPtr, BITS_PER_SAMPLE);

    // VORBIS_COMMENT (タグ) を設定する
    // [NOTE] wasm のメモリは拡張されると buffer が差し替わるため、
    // 文字列はすべて書き込んでからポインタ配列を作る
    const vendor = writeCString('flac-rs wasm example');
    const tagNames = ['TITLE', 'ARTIST'];
    const tagValues = ['WASM encoder example', 'flac-rs'];
    const names = tagNames.map(writeCString);
    const values = tagValues.map(writeCString);
    const nameArrayPtr = flac_alloc(names.length * 4);
    const valueArrayPtr = flac_alloc(values.length * 4);
    new Uint32Array(memory.buffer, nameArrayPtr, names.length).set(
        names.map((s) => s.ptr),
    );
    new Uint32Array(memory.buffer, valueArrayPtr, values.length).set(
        values.map((s) => s.ptr),
    );

    if (
        flac_encoder_set_vorbis_comment(
            encoderPtr,
            vendor.ptr,
            nameArrayPtr,
            valueArrayPtr,
            tagNames.length,
        ) !== FLAC_ERROR_OK
    ) {
        const errorMsg = readCString(flac_encoder_get_last_error(encoderPtr));
        console.error(`Error: Failed to set vorbis comment: ${errorMsg}`);
        flac_encoder_free(encoderPtr);
        process.exit(1);
    }

    // 設定した文字列とポインタ配列は不要になったので解放する
    // (wasm 側で複製されるため、この時点で解放してよい)
    flac_free(vendor.ptr, vendor.size);
    for (const s of [...names, ...values]) {
        flac_free(s.ptr, s.size);
    }
    flac_free(nameArrayPtr, names.length * 4);
    flac_free(valueArrayPtr, values.length * 4);

    if (flac_encoder_initialize(encoderPtr) !== FLAC_ERROR_OK) {
        const errorMsg = readCString(flac_encoder_get_last_error(encoderPtr));
        console.error(`Error: Failed to initialize encoder: ${errorMsg}`);
        flac_encoder_free(encoderPtr);
        process.exit(1);
    }

    // サイン波 (左 440 Hz / 右 660 Hz) を生成してチャンクごとに投入する
    const totalSamples = SAMPLE_RATE * DURATION_SECONDS;
    const chunkBytes = CHUNK_SAMPLES * CHANNELS * 4; // int32_t
    const chunkPtr = flac_alloc(chunkBytes);
    let generated = 0;
    while (generated < totalSamples) {
        const n = Math.min(CHUNK_SAMPLES, totalSamples - generated);

        // [NOTE] wasm のメモリは拡張されると buffer が差し替わるため、
        // ビューは wasm 関数の呼び出しごとに作り直す必要がある
        const chunk = new Int32Array(memory.buffer, chunkPtr, n * CHANNELS);
        for (let i = 0; i < n; i++) {
            const t = (generated + i) / SAMPLE_RATE;
            chunk[i * 2] = Math.round(10000 * Math.sin(2 * Math.PI * 440 * t));
            chunk[i * 2 + 1] = Math.round(10000 * Math.sin(2 * Math.PI * 660 * t));
        }

        if (flac_encoder_push_samples(encoderPtr, chunkPtr, n * CHANNELS) !== FLAC_ERROR_OK) {
            const errorMsg = readCString(flac_encoder_get_last_error(encoderPtr));
            console.error(`Error: Failed to push samples: ${errorMsg}`);
            flac_encoder_free(encoderPtr);
            process.exit(1);
        }
        generated += n;
    }
    flac_free(chunkPtr, chunkBytes);

    // エンコード処理を完了する
    if (flac_encoder_finalize(encoderPtr) !== FLAC_ERROR_OK) {
        const errorMsg = readCString(flac_encoder_get_last_error(encoderPtr));
        console.error(`Error: Failed to finalize encoder: ${errorMsg}`);
        flac_encoder_free(encoderPtr);
        process.exit(1);
    }

    // 完成した FLAC ストリームを取得してファイルに書き込む
    const outputDataPtrPtr = flac_alloc(4); // const uint8_t * (wasm32 ではポインタは 4 バイト)
    const outputSizePtr = flac_alloc(8);    // uint64_t
    if (flac_encoder_get_output(encoderPtr, outputDataPtrPtr, outputSizePtr) !== FLAC_ERROR_OK) {
        const errorMsg = readCString(flac_encoder_get_last_error(encoderPtr));
        console.error(`Error: Failed to get output: ${errorMsg}`);
        flac_encoder_free(encoderPtr);
        process.exit(1);
    }

    // [NOTE] wasm は little endian なので、ホストが big endian の場合に備えて DataView を使用している
    const outputDataPtr = new DataView(memory.buffer, outputDataPtrPtr, 4).getUint32(0, true);
    const outputSize = Number(new DataView(memory.buffer, outputSizePtr, 8).getBigUint64(0, true));
    const outputData = new Uint8Array(memory.buffer, outputDataPtr, outputSize);
    fs.writeFileSync(filePath, outputData);

    flac_free(outputDataPtrPtr, 4);
    flac_free(outputSizePtr, 8);

    console.log(`Encoded ${totalSamples} samples (${CHANNELS} channels, ${BITS_PER_SAMPLE} bits, ${SAMPLE_RATE} Hz) into ${filePath} (${outputSize} bytes)`);

    // リソースを解放する
    flac_encoder_free(encoderPtr);
}

// メイン処理
const args = process.argv.slice(2);
if (args.length === 0) {
    console.error('Usage: node encode.js <output.flac>');
    process.exit(1);
}

encodeFlacFile(args[0]);
