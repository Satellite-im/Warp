use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub enum SignalingTypes {
    Offer,
    Answer,
    Candidate,
}

impl Default for SignalingTypes {
    fn default() -> Self {
        Self::Offer
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct SignalingPayload {
    signaling_type: SignalingTypes,
    data: String,
}

impl Default for SignalingPayload {
    fn default() -> Self {
        Self {
            data: Default::default(),
            signaling_type: Default::default(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl SignalingPayload {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn set_data(&mut self, data: String) {
        self.data = data
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn set_is_offer(&mut self, signaling_type: SignalingTypes) {
        self.signaling_type = signaling_type
    }
}
