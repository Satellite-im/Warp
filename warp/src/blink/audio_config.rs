use anyhow::Result;
use dyn_clone::DynClone;
use futures::channel::oneshot::Sender;
pub trait AudioDeviceConfig: DynClone + Send + Sync {
    /// warning: be sure to use tokio::task::spawn_blocking for this function
    fn test_speaker(&self, done: Sender<()>) -> Result<()>;
    /// warning: be sure to use tokio::task::spawn_blocking for this function
    fn test_microphone(&self, done: Sender<()>) -> Result<()>;

    fn set_speaker(&mut self, device_name: &str);
    fn set_microphone(&mut self, device_name: &str);

    fn microphone_device_name(&self) -> Option<String>;
    fn speaker_device_name(&self) -> Option<String>;

    fn get_available_microphones(&self) -> Result<Vec<String>>;
    fn get_available_speakers(&self) -> Result<Vec<String>>;
}
