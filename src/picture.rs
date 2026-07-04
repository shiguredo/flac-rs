//! PICTURE メタデータブロック (RFC 9639 Section 8.8)
//!
//! オーディオに付随する画像 (カバーアートなど) を保持する。
//! 画像データの代わりに URI を格納することもできる。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{DecodeError, EncodeError};
use crate::metadata::MAX_METADATA_PAYLOAD_SIZE;

/// 画像タイプ (RFC 9639 Section 8.8 Table 13)
///
/// 定義済みの値のみを列挙する。それ以外は予約済み。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureType {
    /// 0: その他
    Other,
    /// 1: 32x32 の PNG ファイルアイコン
    PngFileIcon,
    /// 2: 一般のファイルアイコン
    GeneralFileIcon,
    /// 3: フロントカバー
    FrontCover,
    /// 4: バックカバー
    BackCover,
    /// 5: ライナーノーツページ
    LinerNotesPage,
    /// 6: メディアレーベル (CD / Vinyl / カセットのレーベルなど)
    MediaLabel,
    /// 7: リードアーティスト / リードパフォーマー / ソリスト
    LeadArtist,
    /// 8: アーティストまたはパフォーマー
    Artist,
    /// 9: 指揮者
    Conductor,
    /// 10: バンドまたはオーケストラ
    Band,
    /// 11: 作曲者
    Composer,
    /// 12: 作詞者またはテキストライター
    Lyricist,
    /// 13: 録音場所
    RecordingLocation,
    /// 14: 録音中
    DuringRecording,
    /// 15: 演奏中
    DuringPerformance,
    /// 16: 映画またはビデオのスクリーンキャプチャ
    MovieScreenCapture,
    /// 17: 鮮やかな色の魚 (ID3v2 互換のために維持されている。利用は非推奨)
    BrightColoredFish,
    /// 18: イラスト
    Illustration,
    /// 19: バンドまたはアーティストのロゴタイプ
    BandLogotype,
    /// 20: 出版社またはスタジオのロゴタイプ
    PublisherLogotype,
    /// 21-: 予約済み
    Reserved(u32),
}

impl PictureType {
    /// 数値から画像タイプへ変換する
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => PictureType::Other,
            1 => PictureType::PngFileIcon,
            2 => PictureType::GeneralFileIcon,
            3 => PictureType::FrontCover,
            4 => PictureType::BackCover,
            5 => PictureType::LinerNotesPage,
            6 => PictureType::MediaLabel,
            7 => PictureType::LeadArtist,
            8 => PictureType::Artist,
            9 => PictureType::Conductor,
            10 => PictureType::Band,
            11 => PictureType::Composer,
            12 => PictureType::Lyricist,
            13 => PictureType::RecordingLocation,
            14 => PictureType::DuringRecording,
            15 => PictureType::DuringPerformance,
            16 => PictureType::MovieScreenCapture,
            17 => PictureType::BrightColoredFish,
            18 => PictureType::Illustration,
            19 => PictureType::BandLogotype,
            20 => PictureType::PublisherLogotype,
            other => PictureType::Reserved(other),
        }
    }

    /// 画像タイプを数値へ変換する
    pub fn to_u32(self) -> u32 {
        match self {
            PictureType::Other => 0,
            PictureType::PngFileIcon => 1,
            PictureType::GeneralFileIcon => 2,
            PictureType::FrontCover => 3,
            PictureType::BackCover => 4,
            PictureType::LinerNotesPage => 5,
            PictureType::MediaLabel => 6,
            PictureType::LeadArtist => 7,
            PictureType::Artist => 8,
            PictureType::Conductor => 9,
            PictureType::Band => 10,
            PictureType::Composer => 11,
            PictureType::Lyricist => 12,
            PictureType::RecordingLocation => 13,
            PictureType::DuringRecording => 14,
            PictureType::DuringPerformance => 15,
            PictureType::MovieScreenCapture => 16,
            PictureType::BrightColoredFish => 17,
            PictureType::Illustration => 18,
            PictureType::BandLogotype => 19,
            PictureType::PublisherLogotype => 20,
            PictureType::Reserved(other) => other,
        }
    }
}

/// PICTURE メタデータブロック (RFC 9639 Section 8.8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// 画像タイプ
    pub picture_type: PictureType,
    /// メディアタイプ文字列 (RFC 2046)。データ部が URI の場合は `-->`
    pub media_type: String,
    /// 画像の説明 (UTF-8)
    pub description: String,
    /// 画像の幅 (ピクセル)。不明なら 0
    pub width: u32,
    /// 画像の高さ (ピクセル)。不明なら 0
    pub height: u32,
    /// 色深度 (ビット/ピクセル)。不明なら 0
    pub color_depth: u32,
    /// インデックスカラー画像で使用される色数。非インデックスなら 0
    pub colors: u32,
    /// 画像データ (media_type が `-->` の場合は URI)
    pub data: Vec<u8>,
}

impl Picture {
    /// データ部が画像そのものではなく URI であることを示すメディアタイプ
    pub const URI_MEDIA_TYPE: &'static str = "-->";

    /// データ部が URI か
    pub fn is_uri(&self) -> bool {
        self.media_type == Self::URI_MEDIA_TYPE
    }

    /// PICTURE ペイロードをデコードする
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut pos = 0;
        let read_u32 = |pos: &mut usize| -> Result<u32, DecodeError> {
            let end = pos.checked_add(4).ok_or_else(|| {
                DecodeError::InvalidData(String::from(
                    "picture block length overflows (RFC 9639 Section 8.8)",
                ))
            })?;
            if end > payload.len() {
                return Err(DecodeError::InvalidData(String::from(
                    "picture block is truncated (RFC 9639 Section 8.8)",
                )));
            }
            let value = u32::from_be_bytes(
                payload[*pos..end]
                    .try_into()
                    .expect("4 バイト固定 (実装バグ)"),
            );
            *pos = end;
            Ok(value)
        };
        let read_bytes = |pos: &mut usize, len: usize| -> Result<&[u8], DecodeError> {
            let end = pos.checked_add(len).ok_or_else(|| {
                DecodeError::InvalidData(String::from(
                    "picture block length overflows (RFC 9639 Section 8.8)",
                ))
            })?;
            if end > payload.len() {
                return Err(DecodeError::InvalidData(String::from(
                    "picture block is truncated (RFC 9639 Section 8.8)",
                )));
            }
            let bytes = &payload[*pos..end];
            *pos = end;
            Ok(bytes)
        };

        let picture_type = PictureType::from_u32(read_u32(&mut pos)?);

        let media_type_len = read_u32(&mut pos)? as usize;
        let media_type_bytes = read_bytes(&mut pos, media_type_len)?;
        // メディアタイプは印字可能 ASCII 0x20-0x7E (RFC 9639 Section 8.8)
        if !media_type_bytes.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
            return Err(DecodeError::InvalidData(String::from(
                "picture media type must be printable ASCII (RFC 9639 Section 8.8)",
            )));
        }
        let media_type = String::from_utf8(media_type_bytes.to_vec())
            .expect("印字可能 ASCII は常に有効な UTF-8 (実装バグ)");

        let description_len = read_u32(&mut pos)? as usize;
        let description_bytes = read_bytes(&mut pos, description_len)?;
        let description = String::from_utf8(description_bytes.to_vec()).map_err(|_| {
            DecodeError::InvalidData(String::from(
                "picture description is not valid UTF-8 (RFC 9639 Section 8.8)",
            ))
        })?;

        let width = read_u32(&mut pos)?;
        let height = read_u32(&mut pos)?;
        let color_depth = read_u32(&mut pos)?;
        let colors = read_u32(&mut pos)?;

        let data_len = read_u32(&mut pos)? as usize;
        let data = read_bytes(&mut pos, data_len)?.to_vec();

        if pos != payload.len() {
            return Err(DecodeError::InvalidData(format!(
                "picture block has {} trailing bytes (RFC 9639 Section 8.8)",
                payload.len() - pos
            )));
        }

        Ok(Self {
            picture_type,
            media_type,
            description,
            width,
            height,
            color_depth,
            colors,
            data,
        })
    }

    /// PICTURE ペイロードにエンコードする
    pub fn encode_payload(&self) -> Result<Vec<u8>, EncodeError> {
        if !self.media_type.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return Err(EncodeError::InvalidMetadata(String::from(
                "picture media type must be printable ASCII (RFC 9639 Section 8.8)",
            )));
        }

        let mut out = Vec::new();
        out.extend_from_slice(&self.picture_type.to_u32().to_be_bytes());
        out.extend_from_slice(&(self.media_type.len() as u32).to_be_bytes());
        out.extend_from_slice(self.media_type.as_bytes());
        out.extend_from_slice(&(self.description.len() as u32).to_be_bytes());
        out.extend_from_slice(self.description.as_bytes());
        out.extend_from_slice(&self.width.to_be_bytes());
        out.extend_from_slice(&self.height.to_be_bytes());
        out.extend_from_slice(&self.color_depth.to_be_bytes());
        out.extend_from_slice(&self.colors.to_be_bytes());
        out.extend_from_slice(&(self.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.data);

        if out.len() > MAX_METADATA_PAYLOAD_SIZE {
            return Err(EncodeError::InvalidMetadata(format!(
                "picture payload {} bytes exceeds 24-bit size limit (RFC 9639 Section 8.1)",
                out.len()
            )));
        }
        Ok(out)
    }
}
