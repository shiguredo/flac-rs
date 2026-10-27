//! C API の decoder.rs に対応するモジュール
use c_api::decoder::{FlacDecodedFrame, FlacStreamInfo, FlacVorbisComment, FlacVorbisCommentField};

/// STREAMINFO を JSON 文字列に変換する
///
/// # 引数
///
/// - `stream_info`: 変換対象の `FlacStreamInfo` へのポインタ
///
/// # 戻り値
///
/// JSON 文字列を含む `Vec<u8>` へのポインタ。エラー時は NULL
///
/// 呼び出しもとは不要になったら `flac_vec_free` 関数を使ってこの `Vec` を解放する必要がある
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_stream_info_to_json(
    stream_info: *const FlacStreamInfo,
) -> *mut Vec<u8> {
    if stream_info.is_null() {
        return std::ptr::null_mut();
    }

    let stream_info = unsafe { &*stream_info };
    let json = nojson::json(|f| fmt_json_flac_stream_info(f, stream_info)).to_string();
    Box::into_raw(Box::new(json.into_bytes()))
}

/// デコードしたフレームを JSON 文字列に変換する
///
/// サンプル列そのものは JSON には含めず、wasm メモリ内の位置
/// (`samples_offset` / `sample_count`) だけを含める。
/// 利用側は `Int32Array` などでその位置から直接サンプル列を読み出せる
///
/// # 引数
///
/// - `frame`: 変換対象の `FlacDecodedFrame` へのポインタ
///
/// # 戻り値
///
/// JSON 文字列を含む `Vec<u8>` へのポインタ。エラー時は NULL
///
/// 呼び出しもとは不要になったら `flac_vec_free` 関数を使ってこの `Vec` を解放する必要がある
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_decoded_frame_to_json(
    frame: *const FlacDecodedFrame,
) -> *mut Vec<u8> {
    if frame.is_null() {
        return std::ptr::null_mut();
    }

    let frame = unsafe { &*frame };
    let json = nojson::json(|f| fmt_json_flac_decoded_frame(f, frame)).to_string();
    Box::into_raw(Box::new(json.into_bytes()))
}

fn fmt_json_flac_stream_info(
    f: &mut nojson::JsonFormatter<'_, '_>,
    stream_info: &FlacStreamInfo,
) -> std::fmt::Result {
    f.object(|f| {
        // ブロックサイズとフレームサイズの範囲
        f.member("min_block_size", stream_info.min_block_size)?;
        f.member("max_block_size", stream_info.max_block_size)?;
        f.member("min_frame_size", stream_info.min_frame_size)?;
        f.member("max_frame_size", stream_info.max_frame_size)?;

        // ストリームの形式
        f.member("sample_rate", stream_info.sample_rate)?;
        f.member("channels", stream_info.channels)?;
        f.member("bits_per_sample", stream_info.bits_per_sample)?;
        f.member("total_samples", stream_info.total_samples)?;

        // MD5 チェックサム (16 進文字列、全て 0 は不明を表す)
        let md5: String = stream_info.md5.iter().map(|b| format!("{b:02x}")).collect();
        f.member("md5", md5)?;

        Ok(())
    })
}

fn fmt_json_flac_decoded_frame(
    f: &mut nojson::JsonFormatter<'_, '_>,
    frame: &FlacDecodedFrame,
) -> std::fmt::Result {
    f.object(|f| {
        // サンプル列の wasm メモリ内の位置と要素数
        f.member("samples_offset", frame.samples as usize)?;
        f.member("sample_count", frame.sample_count)?;

        // フレームの形式
        f.member("sample_rate", frame.sample_rate)?;
        f.member("channels", frame.channels)?;
        f.member("bits_per_sample", frame.bits_per_sample)?;
        f.member("first_sample_number", frame.first_sample_number)?;

        Ok(())
    })
}

/// VORBIS_COMMENT を JSON 文字列に変換する
///
/// フィールド名と値は文字列としてそのまま JSON に埋め込まれる
///
/// # 引数
///
/// - `comment`: 変換対象の `FlacVorbisComment` へのポインタ
///
/// # 戻り値
///
/// JSON 文字列を含む `Vec<u8>` へのポインタ。エラー時は NULL
///
/// 呼び出しもとは不要になったら `flac_vec_free` 関数を使ってこの `Vec` を解放する必要がある
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flac_vorbis_comment_to_json(
    comment: *const FlacVorbisComment,
) -> *mut Vec<u8> {
    if comment.is_null() {
        return std::ptr::null_mut();
    }

    let comment = unsafe { &*comment };
    let json = nojson::json(|f| fmt_json_flac_vorbis_comment(f, comment)).to_string();
    Box::into_raw(Box::new(json.into_bytes()))
}

/// C 文字列を `&str` として読む
///
/// NULL ポインタの場合は `None` を返す
unsafe fn c_str_to_str<'a>(ptr: *const std::ffi::c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().ok()
}

fn fmt_json_flac_vorbis_comment(
    f: &mut nojson::JsonFormatter<'_, '_>,
    comment: &FlacVorbisComment,
) -> std::fmt::Result {
    f.object(|f| {
        // vendor 文字列 (不正な UTF-8 なら空文字列として扱う)
        let vendor = unsafe { c_str_to_str(comment.vendor) }.unwrap_or("");
        f.member("vendor", vendor)?;

        // フィールド列。fields が NULL なら空配列
        let fields: &[FlacVorbisCommentField] = if comment.fields.is_null() {
            &[]
        } else {
            // field_count 分の要素が有効であることは呼び出し側の責務
            unsafe { std::slice::from_raw_parts(comment.fields, comment.field_count as usize) }
        };
        f.member(
            "fields",
            nojson::array(|f| {
                for field in fields {
                    f.element(nojson::object(|f| {
                        let name = unsafe { c_str_to_str(field.name) }.unwrap_or("");
                        let value = unsafe { c_str_to_str(field.value) }.unwrap_or("");
                        f.member("name", name)?;
                        f.member("value", value)?;
                        Ok(())
                    }))?;
                }
                Ok(())
            }),
        )?;

        Ok(())
    })
}
