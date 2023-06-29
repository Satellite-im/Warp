use clap::Parser;
use opencv::{highgui, prelude::*, videoio};

#[derive(Parser, Debug)]
struct Args {
    /// name of the file to save
    /// try avi
    output: String,
    /// specifies the codec
    /// try MJPG
    fourcc: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
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
