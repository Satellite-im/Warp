use std::{mem, slice};

// opus::Encoder has separate functions for i16 and f32
// want to use the same struct for both functions. will do some unsafe stuff to accomplish this.
pub struct OpusPacketizer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    num_samples: usize,
    raw_bytes: Vec<u8>,
    // number of samples in a frame
    // todo: is this true? or is it the number of bytes...
    frame_size: usize,
}

impl OpusPacketizer {
    pub fn init(
        frame_size: usize,
        sample_rate: u32,
        channels: opus::Channels,
    ) -> anyhow::Result<Self> {
        let mut buf = Vec::new();
        buf.reserve(frame_size * 4);
        let encoder =
            opus::Encoder::new(sample_rate, channels, opus::Application::Voip).map_err(|e| {
                anyhow::anyhow!("{e}: sample_rate: {sample_rate}, channels: {channels:?}")
            })?;

        Ok(Self {
            encoder,
            num_samples: 0,
            raw_bytes: buf,
            frame_size,
        })
    }

    pub fn packetize_i16(&mut self, sample: i16, out: &mut [u8]) -> anyhow::Result<Option<usize>> {
        // opus::Encoder::encode is using raw pointers under the hood.
        let p: *const i16 = &sample;
        let bp: *const u8 = p as *const _;
        let bs: &[u8] = unsafe { slice::from_raw_parts(bp, mem::size_of::<i16>()) };
        self.raw_bytes.extend_from_slice(bs);
        self.num_samples += 1;
        if self.num_samples == self.frame_size {
            let p: *const i16 = self.raw_bytes.as_ptr() as _;
            let bs: &[i16] =
                unsafe { slice::from_raw_parts(p, mem::size_of::<i16>() * self.num_samples) };
            match self.encoder.encode(bs, out) {
                Ok(size) => {
                    self.raw_bytes.clear();
                    self.num_samples = 0;
                    return Ok(Some(size));
                }
                Err(e) => anyhow::bail!("failed to encode: {e}"),
            }
        } else {
            return Ok(None);
        }
    }

    pub fn packetize_f32(&mut self, sample: f32, out: &mut [u8]) -> anyhow::Result<Option<usize>> {
        // opus::Encoder::encode is using raw pointers under the hood.
        let p: *const f32 = &sample;
        let bp: *const u8 = p as *const _;
        let bs: &[u8] = unsafe { slice::from_raw_parts(bp, mem::size_of::<f32>()) };
        self.raw_bytes.extend_from_slice(bs);
        self.num_samples += 1;
        if self.num_samples == self.frame_size {
            let p: *const f32 = self.raw_bytes.as_ptr() as _;
            let bs: &[f32] =
                unsafe { slice::from_raw_parts(p, mem::size_of::<f32>() * self.num_samples) };
            match self.encoder.encode_float(bs, out) {
                Ok(size) => {
                    self.raw_bytes.clear();
                    self.num_samples = 0;
                    return Ok(Some(size));
                }
                Err(e) => anyhow::bail!("failed to encode: {e}"),
            }
        } else {
            return Ok(None);
        }
    }
}
