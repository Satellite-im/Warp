use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
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
    fn test_speaker(&self) -> Result<()> {
        todo!()
    }

    fn test_microphone(&self) -> Result<()> {
        todo!()
    }

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
}
