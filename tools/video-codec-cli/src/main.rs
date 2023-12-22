// this is a gutted version of open-cv test. At this point the av1 codec has been chosen and all that's needed is to validate eye-rs.

use clap::Parser;
use video_codec_cli::encode::encode_aom;

/// capture a video and encode it using av1 codec, saving the file at the specified path
#[derive(Parser, Debug)]
struct Command {
    output_file: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cmd = Command::parse();
    encode_aom(&cmd.output_file)?;
    Ok(())
}
