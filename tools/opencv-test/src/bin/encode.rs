use anyhow::Result;
use clap::Parser;
use opencv_test::encode::CodecTypes;
use opencv_test::encode::*;

fn main() -> Result<()> {
    let args = opencv_test::encode::Args::parse();

    match args.codec {
        CodecTypes::H264 => encode_h264(args),
        CodecTypes::X264 => encode_x264(args),
        CodecTypes::AV1 => encode_av1(args),
    }
}
