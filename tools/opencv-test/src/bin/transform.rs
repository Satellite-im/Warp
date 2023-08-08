use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

use clap::Parser;
use opencv::{prelude::*, videoio};

// transforms the input file to h264
#[derive(Parser, Debug)]
struct Args {
    /// an mp4 file generated by opencv
    input: String,
    /// name of the file to save
    output: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut cam = videoio::VideoCapture::from_file(&args.input, videoio::CAP_ANY)?;
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to open video file!");
    }

    // https://docs.opencv.org/3.4/d4/d15/group__videoio__flags__base.html
    let frame_width = cam.get(3)? as _;
    let frame_height = cam.get(4)? as _;
    let fps = cam.get(5)? as _;

    let output_file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.output)?;
    let mut writer = BufWriter::new(output_file);

    let config =
        openh264::encoder::EncoderConfig::new(frame_width, frame_height).max_frame_rate(fps); //.rate_control_mode(openh264::encoder::RateControlMode::Timestamp);

    let mut encoder = openh264::encoder::Encoder::with_config(config)?;

    // works great but is GPL :(
    // let mut encoder = x264::Encoder::builder()
    //     .fps(fps as _, 1)
    //     .build(x264::Colorspace::BGR, frame_width as _, frame_height as _)
    //     .expect("failed to make builder");

    // converts from rgb to yuv
    // let m: [[f32; 3]; 3] = [
    //     [0.183, 0.614, 0.062],
    //     [-0.101, -0.339, 0.439],
    //     [0.439, -0.399, -0.040],
    // ];

    // hopefully converts from bgr to yuv.
    let m: [[f32; 3]; 3] = [
        [0.062, 0.614, 0.183],
        [0.439, -0.339, -0.101],
        [-0.040, -0.399, 0.439],
    ];

    let p = m.as_ptr() as *mut std::ffi::c_void;
    let m = unsafe {
        Mat::new_rows_cols_with_data(3, 3, opencv::core::CV_32F, p, opencv::core::Mat_AUTO_STEP)
    }?;

    loop {
        let mut frame = Mat::default();
        if !cam.read(&mut frame)? {
            println!("read entire video file");
            break;
        }
        let mut xformed = Mat::default();
        opencv::core::transform(&frame, &mut xformed, &m)?;
        let sz = frame.size()?;
        if sz.width > 0 {
            // let p = frame.data_mut();
            let len = sz.width * sz.height * 3;
            // let s = std::ptr::slice_from_raw_parts(p, len as _);

            let mut v = Vec::new();
            v.resize(len as _, 0);
            let y_offset = 0;
            let u_offset = (len / 3) as usize;
            let v_offset = u_offset * 2;

            let mut offset = 0;
            for (idx, val) in frame.data_bytes()?.iter().enumerate() {
                match idx % 3 {
                    0 => v[y_offset + offset] = *val + 16,
                    1 => v[u_offset + offset] = *val + 128,
                    2 => {
                        v[v_offset + offset] = *val + 128;
                        offset += 1;
                    }
                    _ => {
                        panic!("should never happen");
                    }
                }
            }

            let yuv_buf = opencv_test::utils::YUVBuf {
                yuv: v,
                width: sz.width as _,
                height: sz.height as _,
            };

            let encoded_stream = encoder.encode(&yuv_buf)?;
            encoded_stream.write(&mut writer)?;

            // let img = x264::Image::bgr(sz.width, sz.height, unsafe { &*s });
            // let (data, _) = encoder
            //     .encode(fps as i64 * idx as i64, img)
            //     .expect("failed to encode frame");
            // idx += 1;
            // writer.write(data.entirety())?;
        }
    }
    writer.flush()?;
    Ok(())
}
