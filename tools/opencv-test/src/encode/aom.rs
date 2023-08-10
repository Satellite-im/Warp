use crate::utils::RGBBuf;

use super::Args;
use anyhow::{bail, Result};
use av_data::{
    frame::FrameType,
    pixel::{self},
    timeinfo::TimeInfo,
};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    sync::Arc,
};

use libaom::encoder::*;

use opencv::{prelude::*, videoio};

pub fn encode_aom(args: Args) -> Result<()> {
    let cam = videoio::VideoCapture::from_file(&args.input, videoio::CAP_ANY)?;
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to open video file!");
    }

    // https://docs.opencv.org/3.4/d4/d15/group__videoio__flags__base.html
    let frame_width = cam.get(3)? as u32;
    let frame_height = cam.get(4)? as u32;
    let _fps = cam.get(5)? as f32;

    let output_file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.output)?;
    let mut writer = BufWriter::new(output_file);

    let mut encoder_config = match AV1EncoderConfig::new_with_usage(AomUsage::RealTime) {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1EncoderConfig: {e:?}"),
    };
    encoder_config.g_h = frame_height;
    encoder_config.g_w = frame_width;
    let mut encoder = match encoder_config.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };
    let pixel_format = Arc::new(pixel::formats::RGB24.clone());
    let mut idx = 0;
    let mut iter = crate::VideoFileIter::new(cam);
    while let Some(mut frame) = iter.next() {
        println!("read new frame");
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

        let buf = Box::new(RGBBuf::from_gbr(s, width, height));

        let frame = av_data::frame::Frame {
            kind: av_data::frame::MediaKind::Video(av_data::frame::VideoInfo {
                width,
                height,
                flipped: false,
                bits: 24,
                frame_type: FrameType::I,
                format: pixel_format.clone(),
            }),
            buf,
            t: TimeInfo {
                pts: Some(idx * 60),
                ..Default::default()
            },
        };

        idx += 1;

        if let Err(e) = encoder.encode(&frame) {
            println!("encoding error: {e}");
        }
    }
    writer.flush()?;
    Ok(())
}
