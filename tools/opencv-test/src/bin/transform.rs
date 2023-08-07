use clap::Parser;
use opencv::{prelude::*, videoio, core::{CV_32F, Mat_AUTO_STEP}};

// https://softron.zendesk.com/hc/en-us/articles/207695697-List-of-FourCC-codes-for-video-codecs
#[derive(Parser, Debug)]
struct Args {
    input: String,
    /// name of the file to save
    output: String,
    /// specifies the codec
    fourcc: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let fourcc = args.fourcc.as_bytes();

    let mut cam = videoio::VideoCapture::from_file(&args.input, videoio::CAP_ANY)?;
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to open video file!");
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
        if !cam.read(&mut frame)? {
            println!("read entire video file");
            break;
        }
        // supposedly [M][RGB]=[YUV]
        let mut m: [[f32; 3]; 3] = [[0.299, 0.587, 0.114], [-0.14713, -0.28886, 0.436], [0.615, -0.51499, -0.10001]];
        let p = m.as_ptr() as *mut std::ffi::c_void;
        let M = unsafe {Mat::new_rows_cols_with_data(3,3,CV_32F, p, Mat_AUTO_STEP)}?; // may need to .inv() this
        let mut xformed = Mat::default();
        opencv::core::transform(&frame, &mut xformed, &M)?;
        if frame.size()?.width > 0 {
            writer.write(&xformed)?;
        }
    }
    writer.release()?;
    Ok(())
}
