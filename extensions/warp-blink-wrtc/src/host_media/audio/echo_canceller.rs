use anyhow::Result;
use warp::{blink, crypto::DID};
use webrtc_audio_processing::{
    EchoCancellation, EchoCancellationSuppressionLevel, NoiseSuppression, NoiseSuppressionLevel,
    Processor as AudioProcessor, VoiceDetection, VoiceDetectionLikelihood,
};

pub const AUDIO_FRAME_SIZE: usize = 480;
pub struct EchoCanceller {
    // when a SinkTrack adds a frame, this is used to determine which Vec of frames gets updated
    sink_ids: Vec<DID>,
    // these come from SinkTracks. they get processed before processing a capture frame.
    render_frames: Vec<Vec<f32>>,
    audio_processor: AudioProcessor,
    initialization_config: webrtc_audio_processing::InitializationConfig,
    audio_processing_config: webrtc_audio_processing::Config,
}

impl EchoCanceller {
    pub fn new(other_participants: Vec<DID>) -> Result<Self> {
        let initialization_config = webrtc_audio_processing::InitializationConfig {
            num_capture_channels: 1,
            num_render_channels: other_participants.len() as i32,
            ..Default::default()
        };
        let audio_processing_config = webrtc_audio_processing::Config::default();
        let audio_processor = AudioProcessor::new(&initialization_config)?;
        let mut render_frames = vec![];
        for _ in 0..other_participants.len() {
            render_frames.push(Vec::from([0.0; AUDIO_FRAME_SIZE]));
        }
        Ok(Self {
            sink_ids: other_participants,
            render_frames,
            audio_processor,
            initialization_config,
            audio_processing_config,
        })
    }
    pub fn config_audio_processor(&mut self, config: blink::AudioProcessingConfig) -> Result<()> {
        let mut audio_processor =
            webrtc_audio_processing::Processor::new(&self.initialization_config)?;

        let echo_config = config.echo.as_ref().map(|config| {
            let suppression_level = match config {
                blink::EchoCancellationConfig::Low => EchoCancellationSuppressionLevel::Low,
                blink::EchoCancellationConfig::Medium => EchoCancellationSuppressionLevel::Moderate,
                blink::EchoCancellationConfig::High => EchoCancellationSuppressionLevel::High,
            };

            EchoCancellation {
                suppression_level,
                stream_delay_ms: None,
                enable_delay_agnostic: true,
                enable_extended_filter: true,
            }
        });

        let noise_suppression = config.noise.as_ref().map(|noise| match noise {
            blink::NoiseSuppressionConfig::High => NoiseSuppression {
                suppression_level: NoiseSuppressionLevel::High,
            },
            blink::NoiseSuppressionConfig::Moderate => NoiseSuppression {
                suppression_level: NoiseSuppressionLevel::Moderate,
            },
            blink::NoiseSuppressionConfig::Low => NoiseSuppression {
                suppression_level: NoiseSuppressionLevel::Low,
            },
        });

        let voice_detection = config.voice.as_ref().map(|voice| {
            let detection_likelihood = match voice {
                blink::VoiceDetectionConfig::High => VoiceDetectionLikelihood::High,
                blink::VoiceDetectionConfig::Moderate => VoiceDetectionLikelihood::Moderate,
                blink::VoiceDetectionConfig::Low => VoiceDetectionLikelihood::Low,
            };
            VoiceDetection {
                detection_likelihood,
            }
        });

        let ap_config = webrtc_audio_processing::Config {
            echo_cancellation: echo_config,
            enable_high_pass_filter: true,
            noise_suppression,
            voice_detection,
            ..Default::default()
        };
        audio_processor.set_config(ap_config.clone());

        self.audio_processing_config = ap_config;
        self.audio_processor = audio_processor;
        Ok(())
    }

    pub fn insert_render_frame(&mut self, sink_id: &DID, frame: &[f32]) -> Result<()> {
        let idx = self
            .sink_ids
            .iter()
            .position(|x| x == sink_id)
            .ok_or(anyhow::anyhow!("id not found"))?;
        self.render_frames[idx] = Vec::from(frame);

        Ok(())
    }

    pub fn process_capture_frame(&mut self, frame: &mut [f32]) -> Result<()> {
        self.audio_processor
            .process_render_frame_noninterleaved(&mut self.render_frames)?;
        self.audio_processor.process_capture_frame(frame)?;

        let mut render_frames = vec![];
        for _ in 0..self.sink_ids.len() {
            render_frames.push(Vec::from([0.0; AUDIO_FRAME_SIZE]));
        }
        self.render_frames = render_frames;
        Ok(())
    }
}
