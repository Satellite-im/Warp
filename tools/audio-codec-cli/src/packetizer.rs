// opus::Encoder has separate functions for i16 and f32
// want to use the same struct for both functions. will do some unsafe stuff to accomplish this.
pub struct OpusPacketizer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    float_samples: Vec<f32>,
    _int_samples: Vec<i16>,
    // number of samples in a frame
    frame_size: usize,
}

impl OpusPacketizer {
    pub fn init(
        frame_size: usize,
        sample_rate: u32,
        channels: opus::Channels,
    ) -> anyhow::Result<Self> {
        let encoder =
            opus::Encoder::new(sample_rate, channels, opus::Application::Voip).map_err(|e| {
                anyhow::anyhow!("{e}: sample_rate: {sample_rate}, channels: {channels:?}")
            })?;

        Ok(Self {
            encoder,
            float_samples: vec![],
            _int_samples: vec![],
            frame_size,
        })
    }

    pub fn _packetize_i16(&mut self, sample: i16, out: &mut [u8]) -> anyhow::Result<Option<usize>> {
        self._int_samples.push(sample);
        if self._int_samples.len() == self.frame_size {
            match self.encoder.encode(self._int_samples.as_slice(), out) {
                Ok(size) => {
                    self._int_samples.clear();
                    Ok(Some(size))
                }
                Err(e) => anyhow::bail!("failed to encode: {e}"),
            }
        } else {
            Ok(None)
        }
    }

    pub fn packetize_f32(&mut self, sample: f32, out: &mut [u8]) -> anyhow::Result<Option<usize>> {
        self.float_samples.push(sample);
        if self.float_samples.len() == self.frame_size {
            match self
                .encoder
                .encode_float(self.float_samples.as_slice(), out)
            {
                Ok(size) => {
                    self.float_samples.clear();
                    Ok(Some(size))
                }
                Err(e) => anyhow::bail!("failed to encode: {e}"),
            }
        } else {
            Ok(None)
        }
    }
}
