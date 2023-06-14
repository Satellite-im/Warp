use anyhow::{bail, Result};
use uuid::Uuid;
use webrtc_audio_processing::Processor as AudioProcessor;

pub const AUDIO_FRAME_SIZE: usize = 480;
pub struct EchoCanceller {
    // when a SinkTrack adds a frame, this is used to determine which Vec of frames gets updated
    sink_ids: Vec<Uuid>,
    // these come from SinkTracks. they get processed before processing a capture frame.
    render_frames: Vec<Vec<f32>>,
    audio_processor: AudioProcessor,
    initialization_config: webrtc_audio_processing::InitializationConfig,
    audio_processing_config: webrtc_audio_processing::Config,
}

impl EchoCanceller {
    pub fn add_sink_track(&mut self) -> Result<Uuid> {
        // order the operations to not modify self in the event of an error
        let mut new_config = self.initialization_config.clone();
        new_config.num_capture_channels += 1;
        let mut ap = AudioProcessor::new(&new_config)?;
        self.initialization_config = new_config;

        ap.set_config(self.audio_processing_config.clone());

        let id = Uuid::new_v4();
        self.sink_ids.push(id);
        self.render_frames.push(Vec::from([0.0; AUDIO_FRAME_SIZE]));

        Ok(id)
    }

    pub fn remove_sink_track(&mut self, sink_id: Uuid) -> Result<()> {
        if self.initialization_config.num_capture_channels == 0 {
            bail!("no tracks to remove");
        }

        let idx = self
            .sink_ids
            .iter()
            .position(|x| x == &sink_id)
            .ok_or(anyhow::anyhow!("id not found"))?;

        let mut new_config = self.initialization_config.clone();
        new_config.num_capture_channels -= 1;
        let mut ap = AudioProcessor::new(&new_config)?;
        self.initialization_config = new_config;
        ap.set_config(self.audio_processing_config.clone());

        self.sink_ids.remove(idx);
        self.render_frames.remove(idx);

        Ok(())
    }

    pub fn insert_render_frame(
        &mut self,
        sink_id: Uuid,
        frame: &[f32; AUDIO_FRAME_SIZE],
    ) -> Result<()> {
        let idx = self
            .sink_ids
            .iter()
            .position(|x| x == &sink_id)
            .ok_or(anyhow::anyhow!("id not found"))?;
        self.render_frames[idx] = Vec::from(*frame);

        Ok(())
    }

    pub fn process_capture_frame(&mut self, frame: &mut [f32; AUDIO_FRAME_SIZE]) -> Result<()> {
        self.audio_processor
            .process_render_frame_noninterleaved(&mut self.render_frames)?;
        self.audio_processor.process_capture_frame(frame)?;
        Ok(())
    }
}
