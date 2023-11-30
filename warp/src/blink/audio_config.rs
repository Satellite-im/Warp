use anyhow::Result;
use futures::channel::oneshot::Sender;
pub trait AudioDeviceConfig: Send + Sync {
    fn test_speaker(&self, done: Sender<()>) -> Result<()>;
    fn test_microphone(&self, done: Sender<()>) -> Result<()>;

    fn set_speaker(&mut self, device_name: &str);
    fn set_microphone(&mut self, device_name: &str);

    fn microphone_device_name(&self) -> Option<String>;
    fn speaker_device_name(&self) -> Option<String>;

    fn get_available_microphones(&self) -> Result<Vec<String>>;
    fn get_available_speakers(&self) -> Result<Vec<String>>;
}
