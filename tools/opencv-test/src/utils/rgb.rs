pub fn bgr_to_rgb(input: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(input);
    for chunk in v.chunks_exact_mut(3) {
        chunk.swap(0, 2);
    }
    v
}

pub struct RGBBuf {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl av_data::frame::FrameBuffer for RGBBuf {
    fn linesize(&self, idx: usize) -> Result<usize, av_data::frame::FrameError> {
        match idx {
            0 | 1 | 2 => Ok(self.width),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn count(&self) -> usize {
        3
    }

    fn as_slice_inner(&self, idx: usize) -> Result<&[u8], av_data::frame::FrameError> {
        match idx {
            0 | 1 | 2 => Ok(&self.data[0..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn as_mut_slice_inner(&mut self, idx: usize) -> Result<&mut [u8], av_data::frame::FrameError> {
        match idx {
            0 | 1 | 2 => Ok(&mut self.data[0..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }
}
