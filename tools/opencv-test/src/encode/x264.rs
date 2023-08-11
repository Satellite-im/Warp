use super::Args;
use anyhow::Result;

use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

use opencv::{prelude::*, videoio};

pub fn encode_x264(args: Args) -> Result<()> {
    let cam = videoio::VideoCapture::from_file(&args.input, videoio::CAP_ANY)?;
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to open video file!");
    }

    // https://docs.opencv.org/3.4/d4/d15/group__videoio__flags__base.html
    let frame_width = cam.get(3)? as i32;
    let frame_height = cam.get(4)? as i32;
    let fps = cam.get(5)?;

    let output_file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.output)?;
    let mut writer = BufWriter::new(output_file);
    let mut encoder = x264::Encoder::builder()
        .fps(fps as _, 1)
        .build(x264::Colorspace::BGR, frame_width as _, frame_height as _)
        .expect("failed to make builder");
    let mut idx = 0;

    for mut frame in crate::VideoFileIter::new(cam) {
        let sz = frame.size()?;
        if sz.width > 0 {
            let p = frame.data_mut();
            let len = sz.width * sz.height * 3;
            let s = std::ptr::slice_from_raw_parts(p, len as _);

            let img = x264::Image::bgr(sz.width, sz.height, unsafe { &*s });
            let (data, _) = encoder
                .encode(fps as i64 * idx as i64, img)
                .expect("failed to encode frame");
            idx += 1;
            let _ = writer.write(data.entirety())?;
        }
    }
    writer.flush()?;
    Ok(())
}
