/// processes the loudness level from RTP packets. This value is a u8.
/// it is created by taking the output of loudness::Calculator (a float), multiplying it by 1000, and casting it as a u8, saturating at 127.
/// it seems that values >= 10 could be speech.
///
/// each RTP packet has a frame size which spans some timeframe, usually 10 or 20 milliseconds.
/// delay is measured in frames.
pub struct Detector {
    min_delay_between_events: usize,
    remaining_delay: usize,
    speech_threshold: u8,
    is_speaking: bool,
}

impl Detector {
    pub fn new(speech_threshold: u8, min_delay: usize) -> Self {
        Self {
            min_delay_between_events: min_delay,
            remaining_delay: 0,
            speech_threshold,
            is_speaking: false,
        }
    }

    pub fn should_emit_event(&mut self, sample: u8) -> bool {
        if self.remaining_delay > 0 {
            self.remaining_delay -= 1;
            return false;
        }

        self.is_speaking = if sample >= self.speech_threshold {
            self.remaining_delay = self.min_delay_between_events;
            true
        } else {
            false
        };
        self.is_speaking
    }

    pub fn is_speaking(&self) -> bool {
        self.is_speaking
    }
}
