/**
 * flac-rs の WASM バインディングの E2E テスト
 *
 * Node.js から実際に wasm を読み込み、エンコード → デコードの
 * ラウンドトリップとメタデータ (VORBIS_COMMENT) の取得を検証する
 *
 * # 使用方法
 *
 * ```bash
 * cargo build -p wasm --target wasm32-unknown-unknown --profile release-wasm
 * node wasm/examples/test.mjs
 * ```
 */

import assert from 'assert';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// FlacError の値 (include/flac.h の FlacError に対応)
const FLAC_ERROR_OK = 0;
const FLAC_ERROR_NOT_FOUND = 7;

const SAMPLE_RATE = 44100;
const CHANNELS = 2;
const BITS_PER_SAMPLE = 16;
const TOTAL_SAMPLES = 20000;

const wasmPath = path.join(
    __dirname,
    '../../target/wasm32-unknown-unknown/release-wasm/flac_wasm.wasm',
);
const wasmBuffer = fs.readFileSync(wasmPath);
const { instance } = await WebAssembly.instantiate(wasmBuffer);

const {
    memory,
    flac_library_version,
    flac_alloc,
    flac_free,
    flac_vec_ptr,
    flac_vec_len,
    flac_vec_free,
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
    flac_decoder_new,
    flac_decoder_free,
    flac_decoder_get_last_error,
    flac_decoder_feed,
    flac_decoder_finish,
    flac_decoder_decode_frame,
    flac_decoder_get_stream_info,
    flac_decoder_get_vorbis_comment,
    flac_decoded_frame_size,
    flac_stream_info_size,
    flac_vorbis_comment_size,
    flac_vorbis_comment_field_size,
    flac_stream_info_to_json,
    flac_decoded_frame_to_json,
    flac_vorbis_comment_to_json,
} = instance.exports;

// 構造体のサイズは wasm 側の関数で取得する
const FRAME_STRUCT_SIZE = flac_decoded_frame_size();
const STREAM_INFO_STRUCT_SIZE = flac_stream_info_size();
const VORBIS_COMMENT_STRUCT_SIZE = flac_vorbis_comment_size();
const VORBIS_COMMENT_FIELD_SIZE = flac_vorbis_comment_field_size();

// NULL 終端の C 文字列を読み取る
function readCString(ptr) {
    const view = new Uint8Array(memory.buffer, ptr);
    let length = 0;
    while (view[length] !== 0) length++;
    return new TextDecoder().decode(view.slice(0, length));
}

// `Vec<u8>` に格納された JSON 文字列を読み取って解放する
function readJSON(vecPtr) {
    assert.notStrictEqual(vecPtr, 0, 'JSON 変換が NULL を返した');
    const ptr = flac_vec_ptr(vecPtr);
    const len = flac_vec_len(vecPtr);
    const bytes = new Uint8Array(memory.buffer, ptr, len);
    const jsonStr = new TextDecoder().decode(bytes);
    flac_vec_free(vecPtr);
    return JSON.parse(jsonStr);
}

// JavaScript の文字列を wasm メモリに NUL 終端付きで書き込む
function writeCString(str) {
    const bytes = new TextEncoder().encode(str);
    const size = bytes.length + 1;
    const ptr = flac_alloc(size);
    new Uint8Array(memory.buffer, ptr, size).set(bytes);
    new Uint8Array(memory.buffer, ptr + bytes.length, 1)[0] = 0;
    return { ptr, size };
}

// テスト用のサンプル列を生成する (チャンネルインターリーブ済み)
function generateSamples(count) {
    const samples = new Int32Array(count);
    for (let i = 0; i < count; i++) {
        // チャンネルごとに異なる波形にして、インターリーブの取り違えを検出できるようにする
        const phase = Math.floor(i / CHANNELS) % 1000;
        samples[i] = i % CHANNELS === 0 ? phase * 10 : -phase * 10;
    }
    return samples;
}

// アラインメントのテスト: flac_alloc が返すポインタが 8 バイト境界にあること
function testAllocAlignment() {
    const sizes = [1, 2, 3, 4, 5, 8, 16, 32, 33, 64, 100, 1000];
    const allocated = [];
    for (const size of sizes) {
        const ptr = flac_alloc(size);
        assert.notStrictEqual(ptr, 0, `flac_alloc(${size}) が NULL を返した`);
        assert.strictEqual(
            ptr % 8,
            0,
            `flac_alloc(${size}) のポインタ ${ptr} が 8 バイト境界にない`,
        );
        allocated.push([ptr, size]);
    }
    for (const [ptr, size] of allocated) {
        flac_free(ptr, size);
    }
    console.log('アラインメントのテストに成功した');
}

// 構造体サイズ取得関数のテスト
function testStructSizes() {
    // C の FlacDecodedFrame は samples(4) + sample_count(4) + sample_rate(4) +
    // first_sample_number(8) + channels(1) + bits_per_sample(1) + padding
    // で 32 バイトになる
    assert.strictEqual(FRAME_STRUCT_SIZE, 32, 'FlacDecodedFrame のサイズが想定と違う');
    assert.strictEqual(STREAM_INFO_STRUCT_SIZE % 8, 0, 'FlacStreamInfo のサイズが 8 の倍数でない');
    assert.strictEqual(VORBIS_COMMENT_STRUCT_SIZE % 4, 0, 'FlacVorbisComment のサイズが 4 の倍数でない');
    assert.strictEqual(VORBIS_COMMENT_FIELD_SIZE, 8, 'FlacVorbisCommentField のサイズが想定と違う');
    console.log(
        `構造体サイズのテストに成功した (frame=${FRAME_STRUCT_SIZE}, ` +
            `stream_info=${STREAM_INFO_STRUCT_SIZE}, ` +
            `vorbis_comment=${VORBIS_COMMENT_STRUCT_SIZE}, ` +
            `vorbis_comment_field=${VORBIS_COMMENT_FIELD_SIZE})`,
    );
}

// タグを付けてエンコードする
function encodeWithMetadata(vendor, tagNames, tagValues, withMetadata) {
    const encoderPtr = flac_encoder_new();
    assert.notStrictEqual(encoderPtr, 0, 'flac_encoder_new が NULL を返した');

    flac_encoder_set_sample_rate(encoderPtr, SAMPLE_RATE);
    flac_encoder_set_channels(encoderPtr, CHANNELS);
    flac_encoder_set_bits_per_sample(encoderPtr, BITS_PER_SAMPLE);

    if (withMetadata) {
        const vendorStr = writeCString(vendor);
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

        const ret = flac_encoder_set_vorbis_comment(
            encoderPtr,
            vendorStr.ptr,
            nameArrayPtr,
            valueArrayPtr,
            tagNames.length,
        );
        if (ret !== FLAC_ERROR_OK) {
            const msg = readCString(flac_encoder_get_last_error(encoderPtr));
            flac_encoder_free(encoderPtr);
            throw new Error(`VORBIS_COMMENT の設定に失敗した: ${msg}`);
        }

        // wasm 側で複製されるため、この時点で解放してよい
        flac_free(vendorStr.ptr, vendorStr.size);
        for (const s of [...names, ...values]) {
            flac_free(s.ptr, s.size);
        }
        flac_free(nameArrayPtr, names.length * 4);
        flac_free(valueArrayPtr, values.length * 4);
    }

    let ret = flac_encoder_initialize(encoderPtr);
    if (ret !== FLAC_ERROR_OK) {
        const msg = readCString(flac_encoder_get_last_error(encoderPtr));
        flac_encoder_free(encoderPtr);
        throw new Error(`エンコーダーの初期化に失敗した: ${msg}`);
    }

    const samples = generateSamples(TOTAL_SAMPLES);
    const samplesPtr = flac_alloc(TOTAL_SAMPLES * 4);
    new Int32Array(memory.buffer, samplesPtr, TOTAL_SAMPLES).set(samples);
    ret = flac_encoder_push_samples(encoderPtr, samplesPtr, TOTAL_SAMPLES);
    flac_free(samplesPtr, TOTAL_SAMPLES * 4);
    if (ret !== FLAC_ERROR_OK) {
        const msg = readCString(flac_encoder_get_last_error(encoderPtr));
        flac_encoder_free(encoderPtr);
        throw new Error(`サンプルの投入に失敗した: ${msg}`);
    }

    ret = flac_encoder_finalize(encoderPtr);
    if (ret !== FLAC_ERROR_OK) {
        const msg = readCString(flac_encoder_get_last_error(encoderPtr));
        flac_encoder_free(encoderPtr);
        throw new Error(`エンコードの完了処理に失敗した: ${msg}`);
    }

    const outPtrPtr = flac_alloc(4);
    const outSizePtr = flac_alloc(8);
    ret = flac_encoder_get_output(encoderPtr, outPtrPtr, outSizePtr);
    if (ret !== FLAC_ERROR_OK) {
        const msg = readCString(flac_encoder_get_last_error(encoderPtr));
        flac_encoder_free(encoderPtr);
        throw new Error(`出力の取得に失敗した: ${msg}`);
    }

    const outDataPtr = new DataView(memory.buffer, outPtrPtr, 4).getUint32(0, true);
    const outSize = Number(new DataView(memory.buffer, outSizePtr, 8).getBigUint64(0, true));
    // エンコーダーを解放すると無効になるため、内容を複製して返す
    const flacBytes = new Uint8Array(memory.buffer, outDataPtr, outSize).slice();

    flac_free(outPtrPtr, 4);
    flac_free(outSizePtr, 8);
    flac_encoder_free(encoderPtr);

    return { flacBytes, samples };
}

// FLAC をデコードして STREAMINFO と VORBIS_COMMENT を取得する
function decodeAndInspect(flacBytes) {
    const decoderPtr = flac_decoder_new();
    assert.notStrictEqual(decoderPtr, 0, 'flac_decoder_new が NULL を返した');

    const dataPtr = flac_alloc(flacBytes.length);
    new Uint8Array(memory.buffer, dataPtr, flacBytes.length).set(flacBytes);
    let ret = flac_decoder_feed(decoderPtr, dataPtr, flacBytes.length);
    flac_free(dataPtr, flacBytes.length);
    if (ret !== FLAC_ERROR_OK) {
        const msg = readCString(flac_decoder_get_last_error(decoderPtr));
        flac_decoder_free(decoderPtr);
        throw new Error(`データの投入に失敗した: ${msg}`);
    }
    flac_decoder_finish(decoderPtr);

    const framePtr = flac_alloc(FRAME_STRUCT_SIZE);
    const decodedSamples = [];
    let streamInfo = null;
    let vorbisComment = null;
    let vorbisCommentError = FLAC_ERROR_OK;

    while (true) {
        ret = flac_decoder_decode_frame(decoderPtr, framePtr);
        if (ret !== FLAC_ERROR_OK) {
            if (ret !== 6) {
                // FLAC_ERROR_NO_MORE_FRAMES 以外は異常
                const msg = readCString(flac_decoder_get_last_error(decoderPtr));
                flac_free(framePtr, FRAME_STRUCT_SIZE);
                flac_decoder_free(decoderPtr);
                throw new Error(`フレームのデコードに失敗した: ${msg}`);
            }
            break;
        }

        const frame = readJSON(flac_decoded_frame_to_json(framePtr));
        // サンプル列は wasm メモリから直接読み出す
        const view = new Int32Array(
            memory.buffer,
            frame.samples_offset,
            frame.sample_count,
        );
        decodedSamples.push(...view);

        if (streamInfo === null) {
            const infoPtr = flac_alloc(STREAM_INFO_STRUCT_SIZE);
            if (flac_decoder_get_stream_info(decoderPtr, infoPtr) === FLAC_ERROR_OK) {
                streamInfo = readJSON(flac_stream_info_to_json(infoPtr));
            }
            flac_free(infoPtr, STREAM_INFO_STRUCT_SIZE);

            const commentPtr = flac_alloc(VORBIS_COMMENT_STRUCT_SIZE);
            vorbisCommentError = flac_decoder_get_vorbis_comment(decoderPtr, commentPtr);
            if (vorbisCommentError === FLAC_ERROR_OK) {
                vorbisComment = readJSON(flac_vorbis_comment_to_json(commentPtr));
            }
            flac_free(commentPtr, VORBIS_COMMENT_STRUCT_SIZE);
        }
    }

    flac_free(framePtr, FRAME_STRUCT_SIZE);
    flac_decoder_free(decoderPtr);

    return { decodedSamples, streamInfo, vorbisComment, vorbisCommentError };
}

// メタデータありのラウンドトリップ
function testRoundtripWithMetadata() {
    const vendor = 'flac-rs wasm test';
    const tagNames = ['TITLE', 'ARTIST', 'ALBUM'];
    const tagValues = ['Test Title', 'Test Artist', 'Test Album'];

    const { flacBytes, samples } = encodeWithMetadata(
        vendor,
        tagNames,
        tagValues,
        true,
    );
    assert.ok(flacBytes.length > 0, 'エンコード結果が空');
    assert.strictEqual(
        new TextDecoder().decode(flacBytes.slice(0, 4)),
        'fLaC',
        'fLaC マーカーがない',
    );

    const { decodedSamples, streamInfo, vorbisComment } =
        decodeAndInspect(flacBytes);

    // サンプル列の完全一致 (ロスレス性)
    assert.strictEqual(
        decodedSamples.length,
        samples.length,
        'デコードしたサンプル数が一致しない',
    );
    for (let i = 0; i < samples.length; i++) {
        assert.strictEqual(
            decodedSamples[i],
            samples[i],
            `サンプル ${i} が一致しない`,
        );
    }

    // STREAMINFO の検証
    assert.strictEqual(streamInfo.sample_rate, SAMPLE_RATE);
    assert.strictEqual(streamInfo.channels, CHANNELS);
    assert.strictEqual(streamInfo.bits_per_sample, BITS_PER_SAMPLE);
    assert.strictEqual(streamInfo.total_samples, TOTAL_SAMPLES / CHANNELS);

    // VORBIS_COMMENT の検証
    assert.notStrictEqual(vorbisComment, null, 'VORBIS_COMMENT が取得できなかった');
    assert.strictEqual(vorbisComment.vendor, vendor, 'vendor が一致しない');
    assert.strictEqual(
        vorbisComment.fields.length,
        tagNames.length,
        'フィールド数が一致しない',
    );
    for (let i = 0; i < tagNames.length; i++) {
        assert.strictEqual(
            vorbisComment.fields[i].name,
            tagNames[i],
            `フィールド ${i} の名前が一致しない`,
        );
        assert.strictEqual(
            vorbisComment.fields[i].value,
            tagValues[i],
            `フィールド ${i} の値が一致しない`,
        );
    }

    console.log(
        `メタデータありのラウンドトリップに成功した ` +
            `(${samples.length} サンプル、${flacBytes.length} バイト、` +
            `フィールド数 ${vorbisComment.fields.length})`,
    );
}

// メタデータなしのラウンドトリップ
function testRoundtripWithoutMetadata() {
    const { flacBytes, samples } = encodeWithMetadata('', [], [], false);
    const { decodedSamples, vorbisCommentError } = decodeAndInspect(flacBytes);

    // サンプル列の完全一致
    assert.strictEqual(decodedSamples.length, samples.length);
    for (let i = 0; i < samples.length; i++) {
        assert.strictEqual(decodedSamples[i], samples[i]);
    }

    // VORBIS_COMMENT がないので NOT_FOUND
    assert.strictEqual(
        vorbisCommentError,
        FLAC_ERROR_NOT_FOUND,
        'VORBIS_COMMENT がないストリームで NOT_FOUND 以外が返った',
    );

    console.log('メタデータなしのラウンドトリップに成功した (NOT_FOUND を確認)');
}

// メイン処理
console.log(`shiguredo_flac version: ${readCString(flac_library_version())}`);

testAllocAlignment();
testStructSizes();
testRoundtripWithMetadata();
testRoundtripWithoutMetadata();

console.log('すべてのテストに成功した');
