use anyhow::Result;
pub trait AudioDeviceConfig: Send + Sync {
    fn test_speaker(&self) -> Result<()>;
    fn test_microphone(&self) -> Result<()>;

    fn set_speaker(&mut self, device_name: &str);
    fn set_microphone(&mut self, device_name: &str);

    fn microphone_device_name(&self) -> Option<String>;
    fn speaker_device_name(&self) -> Option<String>;

    fn get_available_microphones(&self) -> Result<Vec<String>>;
    fn get_available_speakers(&self) -> Result<Vec<String>>;
}
