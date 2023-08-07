use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

use clap::Parser;
use opencv::{core::VecN, prelude::*, videoio};
use openh264::{
    encoder::{Encoder, EncoderConfig},
    formats::YUVSource,
};

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
    let frame_width = cam.get(3)? as i32;
    let frame_height = cam.get(4)? as i32;
    let _fps = cam.get(5)?;

    let output_file = OpenOptions::new()
        .read(false)
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.output)?;
    let mut writer = BufWriter::new(output_file);
    let encoder_config = EncoderConfig::new(frame_width as u32, frame_height as u32);
    let mut h264_encoder = Encoder::with_config(encoder_config)?;
    loop {
        let mut frame = Mat::default();
        if !cam.read(&mut frame)? {
            println!("read entire video file");
            break;
        }

        let wrapper = YuvWrapper::new(frame_width, frame_height, &frame);
        if frame.size()?.width > 0 {
            let encoded = h264_encoder.encode(&wrapper)?;
            writer.write(&encoded.to_vec())?;
        }
    }
    writer.flush()?;
    Ok(())
}

// each pixel is a 3-tuple
struct YuvWrapper {
    width: i32,
    height: i32,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl YuvWrapper {
    fn new(width: i32, height: i32, frame: &Mat) -> Self {
        let mut y = Vec::new();
        y.reserve(width as usize * height as usize);
        let mut u = y.clone();
        let mut v = y.clone();

        // https://web.archive.org/web/20180423091842/http://www.equasys.de/colorconversion.html
        // RGB dot my = Y'
        // RGB dot mu = U
        // RGB dot mv = V
        let my = VecN::<f32, 3>([0.299, 0.587, 0.114]);
        let mu = VecN::<f32, 3>([-0.14713, -0.28886, 0.436]);
        let mv = VecN::<f32, 3>([0.615, -0.51499, -0.10001]);

        let clamp = |x| if x > 255.0 { 255 } else { x as u8 };

        let it: opencv::core::MatIter<'_, opencv::core::VecN<f32, 3>> =
            frame.iter().expect("couldn't get iter");
        for (_pos, pixel) in it {
            // the * 255.0 is to scale it to fill a u8
            let _y: f32 = pixel.mul(my).0.iter().fold(0.0, |acc, x| acc + *x) * 255.0;
            let _u: f32 = pixel.mul(mu).0.iter().fold(0.0, |acc, x| acc + *x) * 255.0;
            let _v: f32 = pixel.mul(mv).0.iter().fold(0.0, |acc, x| acc + *x) * 255.0;

            y.push(clamp(_y));
            u.push(clamp(_u));
            v.push(clamp(_v));
        }

        Self {
            width,
            height,
            y,
            u,
            v,
        }
    }
}

impl YUVSource for YuvWrapper {
    fn width(&self) -> i32 {
        self.width
    }

    fn height(&self) -> i32 {
        self.height
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }

    fn y_stride(&self) -> i32 {
        self.width * self.height
    }

    fn u_stride(&self) -> i32 {
        self.width * self.height
    }

    fn v_stride(&self) -> i32 {
        self.width * self.height
    }
}
