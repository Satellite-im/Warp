mod h264;
mod x264;
pub use crate::encode::h264::encode_h264;
pub use crate::encode::x264::encode_x264;

use clap::Parser;

// transforms the input file to h264
#[derive(Parser, Debug)]
pub struct Args {
    /// an mp4 file generated by opencv
    pub input: String,
    /// name of the file to save
    pub output: String,
    /// The codec to use
    pub codec: CodecTypes,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum CodecTypes {
    /// OpenH264
    H264,
    /// x264
    X264,
}
