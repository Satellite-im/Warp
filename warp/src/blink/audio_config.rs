use anyhow::Result;
use dyn_clone::DynClone;
use futures::channel::oneshot;
use tokio::sync::mpsc;

// Note: CPAL streams aren't Send and have to be run in an async context. It is expected that tokio::task::spawn_blocking may be used to test
// the microphone and speaker. Because of this, the event channel can't be returned by the function but is instead sent over a oneshot channel.
pub trait AudioDeviceConfig: DynClone + Send + Sync {
    fn test_speaker(
        &self,
        rsp: oneshot::Sender<mpsc::UnboundedReceiver<AudioTestEvent>>,
    ) -> Result<()>;
    fn test_microphone(
        &self,
        rsp: oneshot::Sender<mpsc::UnboundedReceiver<AudioTestEvent>>,
    ) -> Result<()>;

    fn set_speaker(&mut self, device_name: &str);
    fn set_microphone(&mut self, device_name: &str);

    fn microphone_device_name(&self) -> Option<String>;
    fn speaker_device_name(&self) -> Option<String>;

    fn get_available_microphones(&self) -> Result<Vec<String>>;
    fn get_available_speakers(&self) -> Result<Vec<String>>;
}

#[derive(Clone, Debug)]
pub enum AudioTestEvent {
    Output { loudness: u8 },
    Input { loudness: u8 },
    Done,
}
