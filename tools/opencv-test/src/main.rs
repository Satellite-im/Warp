use clap::Parser;
use opencv::{highgui, prelude::*, videoio};
use opencv_test::encode::{encode_aom, CodecTypes};
#[cfg(feature = "all")]
use opencv_test::encode::{encode_h264, encode_rav1e, encode_x264};

#[derive(Parser, Debug)]
enum Command {
    /// play the specified video file
    Play(PlayArgs),
    /// capture video in the specified format and save at the specified file
    Capture(CaptureArgs),
    /// read a video file and re-encode using the specified codec
    Encode(opencv_test::encode::Args),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cmd = Command::parse();
    match cmd {
        Command::Encode(args) => match args.codec {
            #[cfg(feature = "all")]
            CodecTypes::H264 => encode_h264(args)?,
            #[cfg(feature = "all")]
            CodecTypes::X264 => encode_x264(args)?,
            #[cfg(feature = "all")]
            CodecTypes::RAV1E => encode_rav1e(args)?,
            CodecTypes::AOM => encode_aom(args)?,
        },
        Command::Capture(args) => capture(args).await?,
        Command::Play(args) => play(args).await?,
    };
    Ok(())
}

// https://softron.zendesk.com/hc/en-us/articles/207695697-List-of-FourCC-codes-for-video-codecs
#[derive(Parser, Debug)]
struct CaptureArgs {
    /// name of the file to save
    /// try avi
    output: String,
    /// specifies the codec
    /// try MJPG
    fourcc: String,
}

async fn capture(args: CaptureArgs) -> anyhow::Result<()> {
    let fourcc = args.fourcc.as_bytes();

    let window = "video capture";
    highgui::named_window(window, highgui::WINDOW_AUTOSIZE)?;
    let mut cam = videoio::VideoCapture::new(0, videoio::CAP_ANY)?; // 0 is the default camera
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to open default camera!");
    }

    // https://docs.opencv.org/3.4/d4/d15/group__videoio__flags__base.html
    let frame_width = cam.get(3)? as i32;
    let frame_height = cam.get(4)? as i32;
    let fps = cam.get(5)?;

    let mut writer = videoio::VideoWriter::default()?;
    writer.open(
        &args.output,
        videoio::VideoWriter::fourcc(
            fourcc[0].into(),
            fourcc[1].into(),
            fourcc[2].into(),
            fourcc[3].into(),
        )?,
        fps,
        opencv::core::Size::new(frame_width, frame_height),
        true,
    )?;

    loop {
        let mut frame = Mat::default();
        cam.read(&mut frame)?;
        if frame.size()?.width > 0 {
            highgui::imshow(window, &frame)?;
            writer.write(&frame)?;
        }

        let key = highgui::wait_key(10)?;
        if key > 0 && key != 255 {
            break;
        }
    }
    writer.release()?;

    //signal::ctrl_c().await?;
    Ok(())
}

#[derive(Parser, Debug)]
struct PlayArgs {
    /// the video file to play
    input: String,
}

async fn play(args: PlayArgs) -> anyhow::Result<()> {
    let window = "video playback";
    highgui::named_window(window, highgui::WINDOW_AUTOSIZE)?;
    let mut cam = videoio::VideoCapture::from_file(&args.input, videoio::CAP_ANY)?;
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to read from file!");
    }

    loop {
        let mut frame = Mat::default();
        cam.read(&mut frame)?;
        if frame.size()?.width > 0 {
            highgui::imshow(window, &frame)?;
        }

        let key = highgui::wait_key(10)?;
        if key > 0 && key != 255 {
            break;
        }
    }

    Ok(())
}
