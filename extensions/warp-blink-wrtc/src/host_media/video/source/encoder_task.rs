use crate::host_media::{video::{FRAME_HEIGHT, FRAME_WIDTH}};

use eye_hal::traits::Stream;
use openh264::{formats::YUVBuffer, encoder::{EncoderConfig, Encoder}, OpenH264API};
use tokio::sync::mpsc::UnboundedSender;

pub struct Args {
    pub stream: eye_hal::platform::Stream<'static>,
    pub stream_descriptor: eye_hal::stream::Descriptor,
    pub tx: UnboundedSender<Vec<u8>>,
}


pub fn run(args: Args) {
    let Args {
        stream,
        stream_descriptor,
        tx,
    } = args;

    let config = EncoderConfig::new(FRAME_WIDTH as _, FRAME_HEIGHT as _);
    let api = OpenH264API::from_source();
    let mut encoder = Encoder::with_config(api, config)?;

    loop {
         // if should_quit.load(Ordering::Relaxed) {
        //     println!("quitting camera capture rx thread");
        //     break;
        // }

        let rgb_frame = match stream.next() {
                Some(f) => match f {
                    Ok(f) => f,
                    Err(e) => {
                        log::error!("error reading frame: {e}");
                        break;
                    }
                },
                None => {
                    log::error!("error reading frame");
                    break;
                }
            };

            let mut shortened_rgb = vec![];
            shortened_rgb.reserve(FRAME_WIDTH * FRAME_HEIGHT * 3);
    
            let row_start = (stream_descriptor.height as usize - FRAME_HEIGHT) / 2;
            let col_start = ((stream_descriptor.width as usize - FRAME_WIDTH) / 2) * 3;
    
            for row in rgb_frame.chunks_exact(stream_descriptor.width as usize * 3).skip(row_start).take(FRAME_HEIGHT) {
                shortened_rgb.extend_from_slice(&row[col_start..col_start + FRAME_WIDTH * 3]);
            }
    
            let mut yuv_buffer = YUVBuffer::new(FRAME_WIDTH, FRAME_HEIGHT);
            yuv_buffer.read_rgb(&shortened_rgb);
    
            // Encode YUV back into H.264.
            let bitstream = match encoder.encode(&yuv_buffer) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("error encoding frame: {e}");
                    continue;
                }
            };
    
            let _ = tx.send(bitstream.to_vec());
    }

}
