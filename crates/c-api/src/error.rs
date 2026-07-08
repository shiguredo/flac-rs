//! shiguredo_flac のエラーをまとめて定義するためのモジュール
//!
//! C API で細かくエラー型が分かれていると煩雑なので、ひとつに集約している
//!
//! エラーの詳細メッセージは各インスタンスの `*_get_last_error()` で取得できる
use shiguredo_flac::error::{DecodeError, EncodeError};

/// 発生する可能性のあるエラーの種類を表現する列挙型
#[repr(C)]
#[expect(non_camel_case_types)]
pub enum FlacError {
    /// エラーが発生しなかったことを示す
    FLAC_ERROR_OK = 0,

    /// 入力引数ないしパラメーターが無効である
    FLAC_ERROR_INVALID_INPUT,

    /// 入力データが破損しているか無効な形式である
    FLAC_ERROR_INVALID_DATA,

    /// 操作に対する内部状態が無効である
    FLAC_ERROR_INVALID_STATE,

    /// 入力データの読み込みが必要である
    FLAC_ERROR_INPUT_REQUIRED,

    /// NULL ポインタが渡された
    FLAC_ERROR_NULL_POINTER,

    /// これ以上デコードするフレームが存在しない
    FLAC_ERROR_NO_MORE_FRAMES,
}

impl From<DecodeError> for FlacError {
    fn from(_e: DecodeError) -> Self {
        // デコードエラーはすべて「入力データが FLAC として不正」を意味する
        // (データ不足はエラーではなく FLAC_ERROR_INPUT_REQUIRED で表現される)
        Self::FLAC_ERROR_INVALID_DATA
    }
}

impl From<EncodeError> for FlacError {
    fn from(_e: EncodeError) -> Self {
        // エンコードエラーはすべて「利用側が渡した設定・サンプルが不正」を意味する
        Self::FLAC_ERROR_INVALID_INPUT
    }
}
