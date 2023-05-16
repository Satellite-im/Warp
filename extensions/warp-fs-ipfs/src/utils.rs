use std::str::FromStr;

use image::ImageFormat;
use mediatype::MediaTypeBuf;
use warp::{error::Error, constellation::file::FileType};

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub enum ExtensionType {
    #[display(fmt = "image/png")]
    PNG,
    #[display(fmt = "image/jpeg")]
    JPG,
    #[display(fmt = "image/svg+xml")]
    SVG,
    #[display(fmt = "image/gif")]
    GIF,
    #[display(fmt = "image/webp")]
    WEBP,
    #[display(fmt = "image/bmp")]
    BMP,
    #[display(fmt = "image/vnd.microsoft.icon")]
    ICO,
    #[display(fmt = "video/mp4")]
    MP4,
    #[display(fmt = "video/x-msvideo")]
    AVI,
    #[display(fmt = "video/webm")]
    WEBM,
    #[display(fmt = "video/x-matroska")]
    MKV,
    #[display(fmt = "application/pdf")]
    PDF,
    #[display(fmt = "application/msword")]
    DOC,
    #[display(fmt = "application/vnd.openxmlformats-officedocument.wordprocessingml.document")]
    DOCX,
    #[display(fmt = "text/plain")]
    TXT,
    #[display(fmt = "application/octet-stream")]
    Other,
}

impl From<&str> for ExtensionType {
    fn from(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" => Self::PNG,
            "jpeg" | "jpg" => Self::JPG,
            "svg" => Self::SVG,
            "gif" => Self::GIF,
            "bmp" => Self::BMP,
            "ico" => Self::ICO,
            "mp4" => Self::MP4,
            "avi" => Self::AVI,
            "webp" => Self::WEBP,
            "webm" => Self::WEBM,
            "mkv" => Self::MKV,
            "pdf" => Self::PDF,
            "txt" => Self::TXT,
            "docx" => Self::DOCX,
            "doc" => Self::DOC,
            _ => Self::Other,
        }
    }
}

impl TryFrom<ExtensionType> for ImageFormat {
    type Error = Error;
    fn try_from(value: ExtensionType) -> Result<Self, Self::Error> {
        match value {
            ExtensionType::JPG => Ok(ImageFormat::Jpeg),
            ExtensionType::PNG => Ok(ImageFormat::Png),
            ExtensionType::GIF => Ok(ImageFormat::Gif),
            ExtensionType::ICO => Ok(ImageFormat::Ico),
            ExtensionType::BMP => Ok(ImageFormat::Bmp),
            ExtensionType::WEBP => Ok(ImageFormat::WebP),
            _ => Err(Error::Unimplemented),
        }
    }
}

impl TryFrom<ExtensionType> for MediaTypeBuf {
    type Error = Error;
    fn try_from(ext: ExtensionType) -> Result<Self, Self::Error> {
        let ty = ext.to_string();
        let media = MediaTypeBuf::from_str(&ty).map_err(anyhow::Error::from)?;
        Ok(media)
    }
}

impl From<ExtensionType> for FileType {
    fn from(ext: ExtensionType) -> Self {
        match ext.try_into() {
            Ok(media) => FileType::Mime(media),
            Err(_) => FileType::Generic,
        }
    }
}