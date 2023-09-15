use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use warp::blink::AudioDeviceConfig;

#[derive(Clone)]
pub struct DeviceConfig {
    // device name
    // defaults to the default device or None
    // if no devices are connected
    pub selected_speaker: Option<String>,
    // device name
    // defaults to the default device or None
    // if no devices are connected
    pub selected_microphone: Option<String>,
}

impl DeviceConfig {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let output_device = host
            .default_output_device()
            .ok_or(anyhow::anyhow!("no default output device"))?;

        let input_device = host
            .default_input_device()
            .ok_or(anyhow::anyhow!("no default input device"))?;

        Ok(Self {
            selected_speaker: Some(output_device.name()?),
            selected_microphone: Some(input_device.name()?),
        })
    }
}

impl AudioDeviceConfig for DeviceConfig {
    fn set_speaker(&mut self, device_name: &str) {
        self.selected_speaker.replace(device_name.to_string());
    }

    fn set_microphone(&mut self, device_name: &str) {
        self.selected_microphone.replace(device_name.to_string());
    }

    fn microphone_device_name(&self) -> Option<String> {
        self.selected_microphone.clone()
    }

    fn speaker_device_name(&self) -> Option<String> {
        self.selected_speaker.clone()
    }

    fn get_available_microphones(&self) -> Result<Vec<String>> {
        let device_iter = cpal::default_host().input_devices()?;
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }

    fn get_available_speakers(&self) -> Result<Vec<String>> {
        let device_iter = cpal::default_host().output_devices()?;
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }

    // stolen from here: https://github.com/RustAudio/cpal/blob/master/examples/beep.rs
    fn test_speaker(&self) -> Result<()> {
        let host = cpal::default_host();
        let device = self.selected_speaker.clone().unwrap_or_default();
        let device = if device == "default" {
            host.default_output_device()
        } else {
            host.output_devices()?
                .find(|x| x.name().map(|y| y == device).unwrap_or(false))
        }
        .ok_or(anyhow::anyhow!("failed to find output device"))?;
        log::debug!("Output device: {}", device.name()?);

        let config = device.default_output_config()?;
        log::debug!("Default output config: {:?}", config);

        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        let cpal_config = cpal::StreamConfig {
            channels: channels as _,
            sample_rate: cpal::SampleRate(sample_rate as _),
            buffer_size: cpal::BufferSize::Default,
        };

        // Produce a sinusoid of maximum amplitude.
        let mut sample_clock = 0f32;
        let mut next_value = move || {
            sample_clock = (sample_clock + 1.0) % sample_rate;
            (sample_clock * 440.0 * 2.0 * std::f32::consts::PI / sample_rate).sin()
        };
        let err_fn = |err| log::error!("an error occurred on stream: {}", err);

        let stream = device.build_output_stream(
            &cpal_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let value: f32 = f32::from_sample(next_value());
                    for sample in frame.iter_mut() {
                        *sample = value;
                    }
                }
            },
            err_fn,
            None,
        )?;
        stream.play()?;

        std::thread::sleep(std::time::Duration::from_millis(1000));
        Ok(())
    }

    // stolen from here: https://github.com/RustAudio/cpal/blob/master/examples/feedback.rs
    fn test_microphone(&self) -> Result<()> {
        let latency_ms = 500.0;
        let host = cpal::default_host();
        let output_device = self.selected_speaker.clone().unwrap_or_default();
        let output_device = if output_device.to_ascii_lowercase() == "default" {
            host.default_output_device()
        } else {
            host.output_devices()?
                .find(|x| x.name().map(|y| y == output_device).unwrap_or(false))
        }
        .ok_or(anyhow::anyhow!("failed to find output device"))?;

        let input_device = self.selected_microphone.clone().unwrap_or_default();
        let input_device = if input_device.to_ascii_lowercase() == "default" {
            host.default_input_device()
        } else {
            host.input_devices()?
                .find(|x| x.name().map(|y| y == input_device).unwrap_or(false))
        }
        .ok_or(anyhow::anyhow!("failed to find input device"))?;

        log::debug!("Using input device: \"{}\"", input_device.name()?);
        log::debug!("Using output device: \"{}\"", output_device.name()?);

        // We'll try and use the same configuration between streams to keep it simple.
        let input_config: cpal::StreamConfig = input_device.default_input_config()?.into();
        #[allow(clippy::redundant_clone)]
        let mut output_config = input_config.clone();
        if !output_device
            .supported_output_configs()?
            .any(|x| x.channels() == input_config.channels)
        {
            output_config.channels = output_device.default_output_config()?.channels();
        }

        // Create a delay in case the input and output devices aren't synced.
        let latency_frames = (latency_ms / 1_000.0) * input_config.sample_rate.0 as f32;
        // currently each frame is set to contain one sample
        let latency_samples = latency_frames as usize;

        // The buffer to share samples
        let ring = ringbuf::HeapRb::<f32>::new(latency_samples * 2);
        let (mut producer, mut consumer) = ring.split();

        // Fill the samples with 0.0 equal to the length of the delay.
        for _ in 0..latency_samples {
            // The ring buffer has twice as much space as necessary to add latency here,
            // so this should never fail
            let _ = producer.push(0.0);
        }

        let mut input_err_disp_once = false;
        let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut input_fell_behind = false;
            for frame in data.chunks(input_config.channels as _) {
                let sum: f32 = frame.iter().sum();
                let avg = sum / input_config.channels as f32;
                if producer.push(avg).is_err() {
                    input_fell_behind = true;
                }
            }
            if input_fell_behind && !input_err_disp_once {
                input_err_disp_once = true;
                log::error!("input stream fell behind: try increasing latency");
            }
        };

        let mut output_err_disp_once = false;
        let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut output_fell_behind = false;
            for frame in data.chunks_mut(output_config.channels as _) {
                if consumer.is_empty() {
                    output_fell_behind = true;
                }
                let value = consumer.pop().unwrap_or_default();
                for sample in frame.iter_mut() {
                    *sample = value;
                }
            }

            if output_fell_behind && !output_err_disp_once {
                output_err_disp_once = true;
                log::error!("output stream fell behind: try increasing latency");
            }
        };

        // Build streams.
        log::debug!(
            "Attempting to build both streams with f32 samples and `{:?}`.",
            input_config
        );

        let err_fn = |err: cpal::StreamError| {
            log::error!("an error occurred on stream: {}", err);
        };

        let input_stream =
            input_device.build_input_stream(&input_config, input_data_fn, err_fn, None)?;
        let output_stream =
            output_device.build_output_stream(&output_config, output_data_fn, err_fn, None)?;
        log::debug!("Successfully built streams.");

        // Play the streams.
        log::debug!(
            "Starting the input and output streams with `{}` milliseconds of latency.",
            latency_ms
        );
        input_stream.play()?;
        output_stream.play()?;

        // Run for 3 seconds before closing.
        log::debug!("Playing for a few seconds... ");
        std::thread::sleep(std::time::Duration::from_millis(3000 + latency_ms as u64));
        drop(input_stream);
        drop(output_stream);
        log::debug!("Done!");
        Ok(())
    }
}
