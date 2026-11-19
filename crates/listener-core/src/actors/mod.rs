pub mod listener;
pub mod recorder;
pub mod root;
pub mod session;
pub mod source;

pub use listener::*;
pub use recorder::*;
pub use root::*;
pub use session::*;
pub use source::*;

#[cfg(target_os = "macos")]
pub const SAMPLE_RATE: u32 = 16 * 1000;
#[cfg(not(target_os = "macos"))]
pub const SAMPLE_RATE: u32 = 16 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    MicOnly,
    SpeakerOnly,
    MicAndSpeaker,
}

impl ChannelMode {
    /// `mic_only` is the in-person mode: capture the microphone and never open
    /// the system-audio tap.
    pub fn determine(onboarding: bool, mic_only: bool) -> Self {
        if onboarding {
            return ChannelMode::SpeakerOnly;
        }
        if mic_only {
            return ChannelMode::MicOnly;
        }

        ChannelMode::MicAndSpeaker
    }
}

#[cfg(test)]
mod channel_mode_tests {
    use super::ChannelMode;

    #[test]
    fn mic_only_selects_mic_only_unless_onboarding() {
        assert_eq!(
            ChannelMode::determine(false, false),
            ChannelMode::MicAndSpeaker
        );
        assert_eq!(ChannelMode::determine(false, true), ChannelMode::MicOnly);
        assert_eq!(ChannelMode::determine(true, true), ChannelMode::SpeakerOnly);
    }
}

#[derive(Clone)]
pub struct AudioChunk {
    pub data: Vec<f32>,
}
