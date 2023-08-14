use crate::{encode::Mode, utils::yuv::*};

use super::{Args, EncodingType};
use anyhow::{bail, Result};
use av_data::{frame::FrameType, timeinfo::TimeInfo};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    sync::Arc,
};

use libaom::encoder::*;

use opencv::{
    core::{Mat_AUTO_STEP, CV_32F},
    prelude::*,
    videoio,
};

pub fn encode_aom(args: Args) -> Result<()> {
    let color_scale = args.color_scale.unwrap_or(ColorScale::HdTv);
    let optimized_mode = args.mode.unwrap_or(Mode::Normal);
    let is_lossy = args
        .encoding_type
        .map(|t| matches!(t, EncodingType::Lossy))
        .unwrap_or(true);
    let multiplier: usize = if is_lossy { 1 } else { 2 };

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
    encoder_config.g_h = frame_height * multiplier as u32;
    encoder_config.g_w = frame_width * multiplier as u32;
    let mut encoder = match encoder_config.get_encoder() {
        Ok(r) => r,
        Err(e) => bail!("failed to get Av1Encoder: {e:?}"),
    };

    // this is for testing an optimized version
    let color_scale_idx = color_scale.to_idx();
    let mut m = [
        // these scales are for turning RGB to YUV. but the input is in BGR.
        Y_SCALE[color_scale_idx],
        U_SCALE[color_scale_idx],
        V_SCALE[color_scale_idx],
    ];
    m[0].reverse();
    m[1].reverse();
    m[2].reverse();
    let p = m.as_ptr() as *mut std::ffi::c_void;
    let m = unsafe { Mat::new_rows_cols_with_data(3, 3, CV_32F, p, Mat_AUTO_STEP) }
        .expect("failed to make xform matrix");

    let pixel_format = *av_data::pixel::formats::YUV420;
    let pixel_format = Arc::new(pixel_format);
    for (idx, mut frame) in crate::VideoFileIter::new(cam).enumerate() {
        println!("read new frame");
        let sz = frame.size()?;
        let width = sz.width as usize;
        let height = sz.height as usize;
        if width == 0 {
            continue;
        }

        let yuv = match optimized_mode {
            Mode::Faster => bgr_to_yuv420_lossy_faster(frame, &m, width, height, color_scale),
            _ => {
                let p = frame.data_mut();
                let len = width * height * 3;
                let s = std::ptr::slice_from_raw_parts(p, len as _);
                let s: &[u8] = unsafe { &*s };

                if is_lossy {
                    bgr_to_yuv420_lossy(s, width, height, color_scale)
                } else {
                    bgr_to_yuv420(s, width, height, color_scale)
                }
            }
        };

        let yuv_buf = YUV420Buf {
            data: yuv,
            width: width * multiplier,
            height: height * multiplier,
        };

        let frame = av_data::frame::Frame {
            kind: av_data::frame::MediaKind::Video(av_data::frame::VideoInfo::new(
                yuv_buf.width,
                yuv_buf.height,
                false,
                FrameType::I,
                pixel_format.clone(),
            )),
            buf: Box::new(yuv_buf),
            t: TimeInfo {
                pts: Some(idx as i64 * 60),
                ..Default::default()
            },
        };

        println!("encoding");
        if let Err(e) = encoder.encode(&frame) {
            bail!("encoding error: {e}");
        }

        println!("calling get_packet");
        while let Some(packet) = encoder.get_packet() {
            if let AOMPacket::Packet(p) = packet {
                let _ = writer.write(&p.data)?;
            }
        }
    }
    writer.flush()?;
    Ok(())
}
