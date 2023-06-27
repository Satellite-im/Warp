use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
};

use anyhow::{bail, Result};
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

#[derive(Clone)]
pub struct AudioPacket {
    // the time when the sample was sent to the speakers
    // DateTime<Utc> to timestamp_micros
    timestamp_us: u64,
    value: AudioSample,
}

impl EchoCanceller {
    // todo: what if webrtc codec has a different sampling rate (than 48khz) and thus has a different frame size?
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
        start_time_us: u64,
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
                    timestamp_us: start_time_us + (idx * US_PER_SAMPLE) as u64,
                    value: sample,
                });
                idx += 1.0;
            }
        }
        Ok(())
    }

    pub fn process_capture_frame(&mut self, start_time_us: u64, frame: &mut [f32]) -> Result<()> {
        let mut rf = self.process_render_frames(start_time_us)?;
        self.audio_processor.process_render_frame(&mut rf)?;
        self.audio_processor.process_capture_frame(frame)?;

        Ok(())
    }

    // just for testing
    #[cfg(test)]
    fn get_render_frame(&self, peer_id: &DID) -> Result<Vec<AudioPacket>> {
        match self.audio_samples.get(peer_id) {
            Some(x) => Ok(x.iter().cloned().collect()),
            None => bail!("peer not found"),
        }
    }

    fn process_render_frames(&mut self, capture_start_time_us: u64) -> Result<Vec<f32>> {
        let mut render_frames: Vec<Vec<f32>> = Vec::new();

        for (_id, v) in self.audio_samples.iter_mut() {
            let mut frame: Vec<f32> = Vec::new();
            frame.reserve(AUDIO_FRAME_SIZE * 2);
            // pad
            if let Some(render_sample) = v.get(0) {
                match render_sample.timestamp_us.cmp(&capture_start_time_us) {
                    Ordering::Greater => {
                        let num_samples_gap = render_sample
                            .timestamp_us
                            .saturating_sub(capture_start_time_us)
                            as f32
                            / US_PER_SAMPLE;

                        let num_samples_gap = (num_samples_gap.round() as usize).saturating_sub(1);
                        let num_samples_gap = std::cmp::max(num_samples_gap, AUDIO_FRAME_SIZE);
                        frame.resize(num_samples_gap, 0.0);
                    }
                    Ordering::Less => {
                        let num_samples_gap =
                            capture_start_time_us.saturating_sub(render_sample.timestamp_us) as f32
                                / US_PER_SAMPLE;

                        let num_samples_gap = num_samples_gap.round() as usize;
                        for _ in 0..num_samples_gap {
                            v.pop_front();
                        }
                    }
                    _ => {}
                }
            }

            let mut remaining = AUDIO_FRAME_SIZE.saturating_sub(frame.len());
            while remaining > 0 {
                remaining -= 1;
                match v.pop_front() {
                    Some(s) => match s.value {
                        AudioSample::Single(x) => frame.push(x),
                        AudioSample::Dual(x, y) => {
                            frame.push(x);
                            frame.push(y)
                        }
                    },
                    None => match self.codec.channels() {
                        1 => {
                            frame.push(0.0);
                        }
                        _ => {
                            frame.push(0.0);
                            frame.push(0.0);
                        }
                    },
                }
            }
            render_frames.push(frame);
        }

        let mut rf = match render_frames.pop() {
            Some(f) => f,
            None => bail!("no render frames to process"),
        };
        while let Some(v) = render_frames.pop() {
            rf = rf.iter().zip(v.iter()).map(|(l, r)| l + r).collect();
        }

        Ok(rf)
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn echo_canceller1() {
        let audio_codec = blink::AudioCodec {
            mime: blink::MimeType::OPUS,
            sample_rate: blink::AudioSampleRate::High,
            channels: 1,
        };
        let peer_id = DID::default();
        let peers = vec![peer_id.clone()];
        let mut echo_canceller =
            EchoCanceller::new(audio_codec, peers).expect("couldn't create echo canceller");

        let render_frame = vec![1.0, 2.0, 3.0, 4.0];
        echo_canceller
            .insert_render_frame(40, &peer_id, &render_frame)
            .unwrap();

        let frame = echo_canceller.get_render_frame(&peer_id).unwrap();
        let timestamps: Vec<u64> = frame.iter().map(|x| x.timestamp_us).collect();
        assert_eq!(timestamps, vec![40, 60, 81, 102]);

        let r = echo_canceller.process_render_frames(0).unwrap();
        let expected = vec![0.0, 1.0, 2.0, 3.0, 4.0];

        let r1: Vec<f32> = r.iter().take(5).cloned().collect();
        assert_eq!(r1, expected);

        echo_canceller
            .insert_render_frame(40, &peer_id, &render_frame)
            .unwrap();
        let r = echo_canceller.process_render_frames(60).unwrap();
        let expected = vec![2.0, 3.0, 4.0, 0.0, 0.0];

        let r1: Vec<f32> = r.iter().take(5).cloned().collect();
        assert_eq!(r1, expected);
    }

    #[test]
    fn echo_canceller2() {
        let audio_codec = blink::AudioCodec {
            mime: blink::MimeType::OPUS,
            sample_rate: blink::AudioSampleRate::High,
            channels: 1,
        };
        let peer_id = DID::default();
        let peers = vec![peer_id.clone()];
        let mut echo_canceller =
            EchoCanceller::new(audio_codec, peers).expect("couldn't create echo canceller");

        let render_frame = vec![1.0, 2.0, 3.0, 4.0];
        echo_canceller
            .insert_render_frame(40, &peer_id, &render_frame)
            .unwrap();

        let frame = echo_canceller.get_render_frame(&peer_id).unwrap();
        let timestamps: Vec<u64> = frame.iter().map(|x| x.timestamp_us).collect();
        assert_eq!(timestamps, vec![40, 60, 81, 102]);

        let r = echo_canceller.process_render_frames(35).unwrap();
        let expected = vec![1.0, 2.0, 3.0, 4.0, 0.0];

        let r1: Vec<f32> = r.iter().take(5).cloned().collect();
        assert_eq!(r1, expected);

        echo_canceller
            .insert_render_frame(40, &peer_id, &render_frame)
            .unwrap();
        let r = echo_canceller.process_render_frames(45).unwrap();
        let expected = vec![1.0, 2.0, 3.0, 4.0, 0.0];

        let r1: Vec<f32> = r.iter().take(5).cloned().collect();
        assert_eq!(r1, expected);
    }

    #[test]
    fn echo_canceller3() {
        let audio_codec = blink::AudioCodec {
            mime: blink::MimeType::OPUS,
            sample_rate: blink::AudioSampleRate::High,
            channels: 1,
        };
        let peer_id = DID::default();
        let peers = vec![peer_id.clone()];
        let mut echo_canceller =
            EchoCanceller::new(audio_codec, peers).expect("couldn't create echo canceller");

        let render_frame = vec![1.0, 2.0, 3.0, 4.0];
        echo_canceller
            .insert_render_frame(40, &peer_id, &render_frame)
            .unwrap();

        let frame = echo_canceller.get_render_frame(&peer_id).unwrap();
        let timestamps: Vec<u64> = frame.iter().map(|x| x.timestamp_us).collect();
        assert_eq!(timestamps, vec![40, 60, 81, 102]);

        let r = echo_canceller.process_render_frames(0).unwrap();
        let expected = vec![0.0, 1.0, 2.0, 3.0, 4.0];

        let r1: Vec<f32> = r.iter().take(5).cloned().collect();
        assert_eq!(r1, expected);

        echo_canceller
            .insert_render_frame(40, &peer_id, &render_frame)
            .unwrap();
        let r = echo_canceller.process_render_frames(65).unwrap();
        let expected = vec![2.0, 3.0, 4.0, 0.0, 0.0];

        let r1: Vec<f32> = r.iter().take(5).cloned().collect();
        assert_eq!(r1, expected);
    }
}
