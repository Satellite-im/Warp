use super::Args;
use crate::utils::yuv::*;
use anyhow::Result;
use rav1e::{prelude::ChromaSampling, *};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

use opencv::{prelude::*, videoio};

pub fn encode_rav1e(args: Args) -> Result<()> {
    let color_scale = args.color_scale.unwrap_or(ColorScale::Full);
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
        // todo: try using 444
        chroma_sampling: ChromaSampling::Cs420,
        ..Default::default()
    };

    let cfg = Config::new().with_encoder_config(enc);
    let mut ctx: Context<u8> = cfg
        .new_context()
        .map_err(|e| anyhow::anyhow!(format!("couldn't make context: {e:?}")))?;

    for mut frame in crate::VideoFileIter::new(cam) {
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

        println!("converting format");
        // note that width and height have doubled
        let yuv = bgr_to_yuv420(s, width, height, color_scale);
        println!("done converting");

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

        println!("sending frame");
        ctx.send_frame(f1)?;

        loop {
            println!("requesting packet");
            match ctx.receive_packet() {
                Ok(packet) => {
                    println!("got packet");
                    let _ = writer.write(&packet.data)?;
                }
                Err(EncoderStatus::Encoded) => {
                    println!("frame encoded");
                }
                Err(e) => {
                    println!("got err: {e:?}");
                    break;
                }
            }
        }
    }
    writer.flush()?;
    Ok(())
}
