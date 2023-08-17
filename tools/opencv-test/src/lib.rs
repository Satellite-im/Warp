use opencv::{
    prelude::Mat,
    videoio::{VideoCapture, VideoCaptureTrait},
};

pub mod encode;
pub mod utils;

pub struct VideoFileIter {
    cam: VideoCapture,
}

impl VideoFileIter {
    pub fn new(cam: VideoCapture) -> Self {
        Self { cam }
    }
}

impl Iterator for VideoFileIter {
    type Item = opencv::prelude::Mat;

    fn next(&mut self) -> Option<Self::Item> {
        let mut frame = Mat::default();
        match self.cam.read(&mut frame) {
            Ok(b) => {
                if b {
                    Some(frame)
                } else {
                    None
                }
            }
            Err(e) => {
                println!("error reading file: {e}");
                None
            }
        }
    }
}
