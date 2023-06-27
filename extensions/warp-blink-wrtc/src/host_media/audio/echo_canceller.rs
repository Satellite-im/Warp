use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
};

use anyhow::Result;
use warp::{blink, crypto::DID};
use webrtc_audio_processing::{
    EchoCancellation, EchoCancellationSuppressionLevel, NoiseSuppression, NoiseSuppressionLevel,
    Processor as AudioProcessor, VoiceDetection, VoiceDetectionLikelihood,
};

use crate::host_media;

use super::sample::AudioSample;

// the frame size is 10ms
pub const AUDIO_FRAME_SIZE: usize = 480;
// microseconds per sample. ~20.833
pub const US_PER_SAMPLE: f32 = 1000.0 / 48.0;

pub struct EchoCanceller {
    audio_samples: HashMap<DID, VecDeque<AudioPacket>>,
    audio_processor: AudioProcessor,
    initialization_config: webrtc_audio_processing::InitializationConfig,
    audio_processing_config: webrtc_audio_processing::Config,
    codec: blink::AudioCodec,
}

pub struct AudioPacket {
    // the time when the sample was sent to the speakers
    // DateTime<Utc> to timestamp_micros
    timestamp_us: i64,
    value: AudioSample,
}

impl EchoCanceller {
    pub fn new(codec: blink::AudioCodec, mut other_participants: Vec<DID>) -> Result<Self> {
        let mut audio_samples = HashMap::new();
        for participant in other_participants.drain(..) {
            let mut v: VecDeque<AudioPacket> = VecDeque::new();
            v.reserve(AUDIO_FRAME_SIZE * 2);
            audio_samples.insert(participant, v);
        }

        let initialization_config = webrtc_audio_processing::InitializationConfig {
            num_capture_channels: codec.channels() as i32,
            num_render_channels: codec.channels() as i32,
            ..Default::default()
        };
        let audio_processing_config = webrtc_audio_processing::Config::default();
        let audio_processor = AudioProcessor::new(&initialization_config)?;

        Ok(Self {
            audio_samples,
            audio_processor,
            initialization_config,
            audio_processing_config,
            codec,
        })
    }

    pub fn insert_render_frame(
        &mut self,
        start_time_us: i64,
        sink_id: &DID,
        frame: &[f32],
    ) -> Result<()> {
        let samples = self
            .audio_samples
            .get_mut(sink_id)
            .ok_or(anyhow::anyhow!("peer id not found"))?;
        let mut sample_builder = host_media::audio::sample::Builder::new(self.codec.clone());
        let mut idx = 0.0;
        for s in frame {
            if let Some(sample) = sample_builder.build(*s) {
                samples.push_back(AudioPacket {
                    // multiplying by a fractional number and then casting should reduce rounding errors.
                    timestamp_us: start_time_us + (idx * US_PER_SAMPLE) as i64,
                    value: sample,
                });
                idx += 1.0;
            }
        }
        Ok(())
    }

    pub fn process_capture_frame(&mut self, start_time_us: i64, frame: &mut [f32]) -> Result<()> {
        let mut render_frames: Vec<Vec<f32>> = Vec::new();

        for (_id, v) in self.audio_samples.iter_mut() {
            let mut frame: Vec<f32> = Vec::new();
            frame.reserve(AUDIO_FRAME_SIZE * 2);
            // pad
            if let Some(audio_sample) = v.get(0) {
                match audio_sample.timestamp_us.cmp(&start_time_us) {
                    Ordering::Greater => {
                        // render_frame starts after capture frame
                        let num_samples_gap = (audio_sample.timestamp_us as f32
                            - start_time_us as f32)
                            / US_PER_SAMPLE;
                        // round up
                        let num_samples_gap = num_samples_gap.ceil() as usize;
                        frame.resize(num_samples_gap, 0.0);
                    }
                    Ordering::Less => {
                        // render_frame starts before capture frame
                        let num_samples_gap = (start_time_us as f32
                            - audio_sample.timestamp_us as f32)
                            / US_PER_SAMPLE;

                        // round down
                        let num_samples_gap = num_samples_gap.floor() as usize;

                        for _ in 0..num_samples_gap {
                            v.pop_front();
                        }
                    }
                    _ => {}
                }
            }

            let remaining = AUDIO_FRAME_SIZE - frame.len();
            let mut prev_time = None;
            for _ in 0..remaining {
                match v.pop_front() {
                    Some(s) => {
                        let pt = s.timestamp_us;
                        if let Some(t) = prev_time {
                            let diff = s.timestamp_us - t;
                            // this may happen if samples are used from two render_frames. Network delays can cause
                            // the gap between render frames to exceed the sample time.
                            if diff as f32 > US_PER_SAMPLE {
                                v.push_front(s);
                                frame.push(0.0);
                            } else {
                                match s.value {
                                    AudioSample::Single(x) => frame.push(x),
                                    AudioSample::Dual(x, y) => {
                                        frame.push(x);
                                        frame.push(y)
                                    }
                                }
                            }
                        } else {
                            match s.value {
                                AudioSample::Single(x) => frame.push(x),
                                AudioSample::Dual(x, y) => {
                                    frame.push(x);
                                    frame.push(y)
                                }
                            }
                        }
                        prev_time = Some(pt);
                    }
                    None => frame.push(0.0),
                }
            }
            render_frames.push(frame);
        }

        let mut rf = match render_frames.pop() {
            Some(f) => f,
            None => return Ok(()),
        };
        while let Some(v) = render_frames.pop() {
            rf = rf.iter().zip(v.iter()).map(|(l, r)| l + r).collect();
        }
        self.audio_processor.process_render_frame(&mut rf)?;
        self.audio_processor.process_capture_frame(frame)?;

        Ok(())
    }
}

impl EchoCanceller {
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
}

/*
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
*/
