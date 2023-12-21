use crate::utils::yuv::*;

use anyhow::{bail, Result};
use av_data::{frame::FrameType, timeinfo::TimeInfo};
use eye::{
    colorconvert::Device,
    hal::{
        format::PixelFormat,
        traits::{Context as _, Device as _, Stream as _},
        PlatformContext,
    },
};
use std::{sync::mpsc, time::{Instant, Duration}};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    sync::Arc,
};

use libaom::encoder::*;

pub fn encode_aom(output_file: &str) -> Result<()> {
    // Create a context
    let ctx = PlatformContext::all()
        .next()
        .ok_or(anyhow::anyhow!("No platform context available"))?;

    // Create a list of valid capture devices in the system.
    let dev_descrs = ctx.devices()?;

    // Print the supported formats for each device.
    let dev = ctx.open_device(&dev_descrs[0].uri)?;
    let dev = Device::new(dev)?;
    let stream_descr = dev
        .streams()?
        .into_iter()
        .reduce(|s1, s2| {
            // Choose RGB with 8 bit depth
            if s1.pixfmt == PixelFormat::Rgb(24) && s2.pixfmt != PixelFormat::Rgb(24) {
                return s1;
            }

            // Strive for HD (1280 x 720)
            let distance = |width: u32, height: u32| {
                f32::sqrt(((640 - width as i32).pow(2) + (480 - height as i32).pow(2)) as f32)
            };

            if distance(s1.width, s1.height) < distance(s2.width, s2.height) {
                s1
            } else {
                s2
            }
        })
        .ok_or(anyhow::anyhow!("failed to get video stream"))?;

    if stream_descr.pixfmt != PixelFormat::Rgb(24) {
        bail!("No RGB3 streams available");
    }

    println!("Selected stream:\n{:?}", stream_descr);

    // Start the stream
    let mut stream = dev.start_stream(&stream_descr)?;
    let (tx, rx) = mpsc::channel();

    println!("starting stream with description: {stream_descr:?}");

    std::thread::spawn(move || loop {
        let buf = stream.next().unwrap().unwrap();
        if tx.send(buf.to_vec()).is_err() {
            return;
        }
    });

    let color_scale = ColorScale::HdTv;
    let multiplier: usize = 1;

    let frame_width = stream_descr.width;
    let frame_height = stream_descr.height;
    let fps = 1000.0 / (stream_descr.interval.as_millis() as f64);

    let output_file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output_file)?;
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

    let pixel_format = *av_data::pixel::formats::YUV420;
    let pixel_format = Arc::new(pixel_format);

    // run the loop for about 10 seconds
    let start_time = Instant::now();
    while let Ok(frame) = rx.recv() {
       // println!("read new frame");

       let frame_time = Duration::from(Instant::now() - start_time);
       let frame_time_ms = frame_time.as_millis();

       if frame_time_ms > 10000 {
        break;
       }

       let timestamp = frame_time_ms as f64 / fps;

        let yuv = {
            let p = frame.as_ptr();
            let len = frame_width * frame_height * 3;
            let s = std::ptr::slice_from_raw_parts(p, len as _);
            let s: &[u8] = unsafe { &*s };

            rgb_to_yuv420(s, frame_width as _, frame_height as _, color_scale)
        };

        let yuv_buf = YUV420Buf {
            data: yuv,
            width: frame_width as usize * multiplier,
            height: frame_height as usize * multiplier,
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
                pts: Some(timestamp as i64),
                ..Default::default()
            },
        };

       // println!("encoding");
        if let Err(e) = encoder.encode(&frame) {
            bail!("encoding error: {e}");
        }

       // println!("calling get_packet");
        while let Some(packet) = encoder.get_packet() {
            if let AOMPacket::Packet(p) = packet {
                let _ = writer.write(&p.data)?;
            }
        }
    }
    writer.flush()?;
    Ok(())
}
