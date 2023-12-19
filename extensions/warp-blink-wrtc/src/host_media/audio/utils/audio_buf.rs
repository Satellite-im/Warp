pub struct AudioBuf {
    samples: Vec<f32>,
    frame_size: usize,
}

impl AudioBuf {
    pub fn new(frame_size: usize) -> Self {
        let samples = Vec::with_capacity(frame_size);
        Self {
            samples,
            frame_size,
        }
    }

    pub fn insert(&mut self, buf: &[f32]) {
        self.samples.extend_from_slice(buf);
    }

    pub fn get_frame(&mut self) -> Option<Vec<f32>> {
        if self.samples.len() < self.frame_size {
            return None;
        }

        let mut r = vec![0_f32; self.frame_size];
        r.copy_from_slice(&self.samples[0..self.frame_size]);
        let remaining = self.samples.len() - self.frame_size;
        let mut new_samples = vec![0_f32; remaining];
        new_samples.copy_from_slice(&self.samples[self.frame_size..]);
        self.samples = new_samples;
        self.samples.reserve(self.frame_size);

        Some(r)
    }

    pub fn copy_to_slice(&mut self, slice: &mut [f32]) {
        if self.samples.len() < slice.len() {
            slice.fill(0_f32);
            return;
        }
        slice.copy_from_slice(&self.samples[0..slice.len()]);
        let mut samples2 = vec![0_f32; self.samples.len() - slice.len()];
        samples2.copy_from_slice(&self.samples[slice.len()..]);
        samples2.reserve(self.frame_size);
        self.samples = samples2;
    }

    pub fn frame_size(&self) -> usize {
        self.frame_size
    }
}
