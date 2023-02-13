//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!

use std::collections::HashMap;

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use uuid::Uuid;
use warp::{
    blink::{Blink, BlinkEventStream, CallConfig},
    crypto::DID,
    error::Error,
};

mod simple_webrtc;

// todo: add option to init WebRtc using a configuration file
pub struct WebRtc {
    current_call: Option<Call>,
    pending_calls: HashMap<Uuid, Call>,
    cpal_host: cpal::Host,
    audio_input: Option<cpal::Device>,
    audio_output: Option<cpal::Device>,
}

struct Call {
    participants: Vec<DID>,
    config: CallConfig,
    state: CallState,
}

enum CallState {
    Pending,
    InProgress,
    Ended,
}

#[async_trait]
impl Blink for WebRtc {
    // ------ Misc ------
    /// The event stream notifies the UI of call related events
    async fn get_event_stream(&mut self) -> Result<BlinkEventStream, Error> {
        todo!()
    }

    // ------ Create/Join a call ------

    /// attempt to initiate a call. Only one call may be offered at a time.
    /// cannot offer a call if another call is in progress.
    /// During a call, WebRTC connections should only be made to
    /// peers included in the Vec<DID>.
    async fn offer_call(
        &mut self,
        participants: Vec<DID>,
        // default codecs for each type of stream
        config: CallConfig,
    ) -> Result<(), Error> {
        if let Some(_call) = self.current_call.as_ref() {
            // todo: end call
        }

        let new_call = Call {
            participants,
            config,
            state: CallState::Pending,
        };
        self.current_call = Some(new_call);

        // todo: signal
        todo!()
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(_call) = self.current_call.as_ref() {
            // todo: end call
        }

        if let Some(mut call) = self.pending_calls.remove(&call_id) {
            call.state = CallState::InProgress;
            self.current_call = Some(call);
        }
        // todo: signal?
        todo!()
    }
    /// notify a sender/group that you will not join a call
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(mut _call) = self.pending_calls.remove(&call_id) {
            // todo: signal
        }
        todo!()
    }
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error> {
        if let Some(_call) = self.current_call.take() {
            // todo: leave call
        }
        todo!()
    }

    // ------ Select input/output devices ------

    async fn get_available_microphones(&self) -> Result<Vec<String>, Error> {
        let device_iter = match self.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn select_microphone(&mut self, device_name: &str) -> Result<(), Error> {
        let device_iter = match self.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };

        let device = match device_iter
            .filter(|d| d.name().unwrap_or_default() == device_name)
            .next()
        {
            Some(d) => d,
            None => return Err(Error::DeviceNotFound),
        };

        self.audio_input = Some(device);
        Ok(())
    }
    async fn get_available_speakers(&self) -> Result<Vec<String>, Error> {
        let device_iter = match self.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn select_speaker(&mut self, device_name: &str) -> Result<(), Error> {
        let device_iter = match self.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };

        let device = match device_iter
            .filter(|d| d.name().unwrap_or_default() == device_name)
            .next()
        {
            Some(d) => d,
            None => return Err(Error::DeviceNotFound),
        };

        self.audio_output = Some(device);
        Ok(())
    }
    async fn get_available_cameras(&self) -> Result<Vec<String>, Error> {
        todo!()
    }
    async fn select_camera(&mut self, _device_name: &str) -> Result<(), Error> {
        todo!()
    }

    // ------ Media controls ------

    async fn mute_self(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn unmute_self(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn enable_camera(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn disable_camera(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn record_call(&mut self, _output_file: &str) -> Result<(), Error> {
        todo!()
    }
    async fn stop_recording(&mut self) -> Result<(), Error> {
        todo!()
    }

    // ------ Utility Functions ------

    /// Returns the ID of the current call, or None if
    /// a call is not in progress
    fn get_call_id(&self) -> Option<Uuid> {
        todo!()
    }
}
