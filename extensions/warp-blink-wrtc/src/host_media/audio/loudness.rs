/// calculates loudness using root mean square.
/// is basically a moving average filter. has a delay (in samples) equal to the buffer size
pub struct Calculator {
    buf: Vec<f32>,
    ss: f32,
    idx: usize,
    // equals 1/buf.size(). multiplication is faster than division
    normalizer: f32,
}

impl Calculator {
    pub fn new(buf_size: usize) -> Self {
        let mut buf = Vec::new();
        buf.resize(buf_size, 0.0);
        Self {
            buf,
            ss: 0.0,
            idx: 0,
            normalizer: 1.0 / buf_size as f32,
        }
    }
    pub fn insert(&mut self, sample: f32) {
        let sq = sample.powf(2.0);
        self.ss += sq;
        self.ss -= self.buf[self.idx];
        self.buf[self.idx] = sq;
        self.idx = (self.idx + 1) % self.buf.len();
    }

    pub fn get_rms(&self) -> f32 {
        f32::sqrt(self.ss * self.normalizer)
    }

    pub fn reset(&mut self) {
        let mut buf = Vec::new();
        buf.resize(self.buf.len(), 0.0);
        self.buf = buf;
    }
}
