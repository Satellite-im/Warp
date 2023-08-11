use super::Args;
use anyhow::Result;

use crate::utils::yuv::*;
use opencv::{prelude::*, videoio};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

pub fn encode_h264(args: Args) -> Result<()> {
    let color_scale = args.color_scale.unwrap_or(ColorScale::Full);
    let cam = videoio::VideoCapture::from_file(&args.input, videoio::CAP_ANY)?;
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to open video file!");
    }

    // https://docs.opencv.org/3.4/d4/d15/group__videoio__flags__base.html
    let frame_width = cam.get(3)? as u32;
    let frame_height = cam.get(4)? as u32;
    let fps = cam.get(5)? as _;

    let output_file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.output)?;
    let mut writer = BufWriter::new(output_file);

    let config = openh264::encoder::EncoderConfig::new(frame_width * 2, frame_height * 2)
        .max_frame_rate(fps); //.rate_control_mode(openh264::encoder::RateControlMode::Timestamp);

    let mut encoder = openh264::encoder::Encoder::with_config(config)?;

    for mut frame in crate::VideoFileIter::new(cam) {
        let sz = frame.size()?;
        let width = sz.width as usize;
        let height = sz.height as usize;
        if width == 0 {
            continue;
        }
        let p = frame.data_mut();
        let len = width * height * 3;
        let s = std::ptr::slice_from_raw_parts(p, len as _);
        let s: &[u8] = unsafe { &*s };

        let yuv = bgr_to_yuv420(s, width, height, color_scale);

        let yuv_buf = YUV420Buf {
            data: yuv,
            width: width * 2,
            height: height * 2,
        };

        let encoded_stream = encoder.encode(&yuv_buf)?;
        encoded_stream.write(&mut writer)?;
    }
    writer.flush()?;
    Ok(())
}
