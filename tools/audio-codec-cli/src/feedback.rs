use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate,
};
use ringbuf::HeapRb;
use webrtc_audio_processing::{
    EchoCancellation, EchoCancellationSuppressionLevel, InitializationConfig,
};

use crate::{err_fn, StaticArgs};

// taken from here: https://github.com/RustAudio/cpal/blob/master/examples/feedback.rs
pub async fn feedback(args: StaticArgs) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let latency = 1000.0;

    // Find devices.
    let input_device = host.default_input_device().unwrap();

    let output_device = host.default_output_device().unwrap();

    println!("Using input device: \"{}\"", input_device.name()?);
    println!("Using output device: \"{}\"", output_device.name()?);

    // We'll try and use the same configuration between streams to keep it simple.
    let config: cpal::StreamConfig = cpal::StreamConfig {
        channels: 1,
        sample_rate: SampleRate(args.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // Create a delay in case the input and output devices aren't synced.
    let latency_frames = (latency / 1_000.0) * config.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * config.channels as usize;

    // The buffer to share samples
    let ring = HeapRb::<f32>::new(latency_samples * 2);
    let (mut producer, mut consumer) = ring.split();

    // Fill the samples with 0.0 equal to the length of the delay.
    for _ in 0..latency_samples {
        // The ring buffer has twice as much space as necessary to add latency here,
        // so this should never fail
        producer.push(0.0).unwrap();
    }

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let mut output_fell_behind = false;
        for &sample in data {
            if producer.push(sample).is_err() {
                output_fell_behind = true;
            }
        }
        if output_fell_behind {
            eprintln!("output stream fell behind: try increasing latency");
        }
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut input_fell_behind = false;
        for sample in data {
            *sample = match consumer.pop() {
                Some(s) => s,
                None => {
                    input_fell_behind = true;
                    0.0
                }
            };
        }
        if input_fell_behind {
            eprintln!("input stream fell behind: try increasing latency");
        }
    };

    // Build streams.
    println!(
        "Attempting to build both streams with f32 samples and `{:?}`.",
        config
    );
    let input_stream = input_device.build_input_stream(&config, input_data_fn, err_fn, None)?;
    let output_stream = output_device.build_output_stream(&config, output_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    // Play the streams.
    println!(
        "Starting the input and output streams with `{}` milliseconds of latency.",
        latency
    );
    input_stream.play()?;
    output_stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 3 seconds... ");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    drop(input_stream);
    drop(output_stream);
    println!("Done!");
    Ok(())
}

pub async fn echo(args: StaticArgs) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let input_device = host.default_input_device().unwrap();
    let output_device = host.default_output_device().unwrap();

    println!("Using input device: \"{}\"", input_device.name()?);
    println!("Using output device: \"{}\"", output_device.name()?);

    // We'll try and use the same configuration between streams to keep it simple.
    let config: cpal::StreamConfig = cpal::StreamConfig {
        channels: 1,
        sample_rate: SampleRate(args.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // The buffer to share samples
    let ring = HeapRb::<f32>::new(48000 * 2);
    let (mut producer, mut consumer) = ring.split();

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let mut output_fell_behind = false;
        for &sample in data {
            if producer.push(sample).is_err() {
                output_fell_behind = true;
            }
        }
        if output_fell_behind {
            eprintln!("output stream fell behind: try increasing latency");
        }
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut input_fell_behind = false;
        for sample in data {
            *sample = match consumer.pop() {
                Some(s) => s,
                None => {
                    input_fell_behind = true;
                    0.0
                }
            };
        }
        if input_fell_behind {
            eprintln!("input stream fell behind: try increasing latency");
        }
    };

    // Build streams.
    println!(
        "Attempting to build both streams with f32 samples and `{:?}`.",
        config
    );
    let input_stream = input_device.build_input_stream(&config, input_data_fn, err_fn, None)?;
    let output_stream = output_device.build_output_stream(&config, output_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    input_stream.play()?;
    output_stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 10 seconds... ");
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    drop(input_stream);
    drop(output_stream);
    println!("Done!");
    Ok(())
}

pub async fn echo_cancellation(args: StaticArgs) -> anyhow::Result<()> {
    let mut frame: Vec<f32> = Vec::new();
    frame.reserve(480);

    let mut processor = webrtc_audio_processing::Processor::new(&InitializationConfig {
        num_capture_channels: 1,
        num_render_channels: 1,
        ..Default::default()
    })?;
    let config = webrtc_audio_processing::Config {
        echo_cancellation: Some(EchoCancellation {
            suppression_level: EchoCancellationSuppressionLevel::Moderate,
            stream_delay_ms: None,
            enable_delay_agnostic: true,
            enable_extended_filter: true,
        }),
        enable_high_pass_filter: true,
        ..Default::default()
    };
    processor.set_config(config);

    let host = cpal::default_host();
    let input_device = host.default_input_device().unwrap();
    let output_device = host.default_output_device().unwrap();

    println!("Using input device: \"{}\"", input_device.name()?);
    println!("Using output device: \"{}\"", output_device.name()?);

    // We'll try and use the same configuration between streams to keep it simple.
    let config: cpal::StreamConfig = cpal::StreamConfig {
        channels: 1,
        sample_rate: SampleRate(args.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // The buffer to share samples
    let ring = HeapRb::<f32>::new(48000 * 2);
    let (mut producer, mut consumer) = ring.split();

    // latency
    for _ in 0..480 {
        let _ = producer.push(0.0);
    }

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let mut output_fell_behind = false;
        for &sample in data {
            frame.push(sample);
            if frame.len() == 480 {
                if let Err(e) = processor.process_capture_frame(frame.as_mut_slice()) {
                    eprintln!("failed to process capture frame: {e}");
                }
                if let Err(e) = processor.process_render_frame(frame.as_mut_slice()) {
                    eprintln!("failed to process render frame: {e}");
                }
                //let stats = processor.get_stats();
                //println!("rms is: {:?}", stats.rms_dbfs);
                for sample in frame.drain(..) {
                    if producer.push(sample).is_err() {
                        output_fell_behind = true;
                    }
                }
            }
        }
        if output_fell_behind {
            eprintln!("output stream fell behind: try increasing latency");
        }
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut input_fell_behind = false;
        for sample in data {
            *sample = match consumer.pop() {
                Some(s) => s,
                None => {
                    input_fell_behind = true;
                    0.0
                }
            };
        }
        if input_fell_behind {
            eprintln!("input stream fell behind: try increasing latency");
        }
    };

    // Build streams.
    println!(
        "Attempting to build both streams with f32 samples and `{:?}`.",
        config
    );
    let input_stream = input_device.build_input_stream(&config, input_data_fn, err_fn, None)?;
    let output_stream = output_device.build_output_stream(&config, output_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    input_stream.play()?;
    output_stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 10 seconds... ");
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    drop(input_stream);
    drop(output_stream);
    println!("Done!");
    Ok(())
}
