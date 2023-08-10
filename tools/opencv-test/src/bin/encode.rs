use anyhow::Result;
use clap::Parser;
use opencv_test::encode::CodecTypes;

fn main() -> Result<()> {
    let args = opencv_test::encode::Args::parse();

    match args.codec {
        CodecTypes::H264 => opencv_test::encode::encode_h264(args),
        CodecTypes::X264 => opencv_test::encode::encode_x264(args),
    }
}
