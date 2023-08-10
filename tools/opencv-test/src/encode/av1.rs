use super::Args;
use anyhow::Result;
use rav1e::*;

use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

use opencv::{prelude::*, videoio};

pub fn encode_av1(args: Args) -> Result<()> {
    let cam = videoio::VideoCapture::from_file(&args.input, videoio::CAP_ANY)?;
    let opened = videoio::VideoCapture::is_opened(&cam)?;
    if !opened {
        panic!("Unable to open video file!");
    }

    // https://docs.opencv.org/3.4/d4/d15/group__videoio__flags__base.html
    let frame_width = cam.get(3)? as usize;
    let frame_height = cam.get(4)? as usize;
    let _fps = cam.get(5)? as f32;

    let output_file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.output)?;
    let mut writer = BufWriter::new(output_file);

    let enc = EncoderConfig {
        width: frame_width * 2,
        height: frame_height * 2,
        ..Default::default()
    };

    let cfg = Config::new().with_encoder_config(enc);
    let mut ctx: Context<u8> = cfg
        .new_context()
        .map_err(|e| anyhow::anyhow!(format!("couldn't make context: {e:?}")))?;

    let mut iter = crate::VideoFileIter::new(cam);
    while let Some(mut frame) = iter.next() {
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

        // note that width and height have doubled
        let yuv = crate::utils::bgr_to_yuv_lossy(s, width, height);

        // create a frame
        let mut f1 = ctx.new_frame();
        let mut start = 0;
        let mut end = 4 * width * height;
        f1.planes[0].copy_from_raw_u8(&yuv[start..end], width * 2, 1);
        start = end;
        end = start + (width * height);
        f1.planes[1].copy_from_raw_u8(&yuv[start..end], width, 1);
        start = end;
        f1.planes[2].copy_from_raw_u8(&yuv[start..], width, 1);

        ctx.send_frame(f1)?;

        while let Ok(packet) = ctx.receive_packet() {
            writer.write(&packet.data)?;
        }
    }
    writer.flush()?;
    Ok(())
}
