//! Audio encoding for recordings, exports and transcription uploads: AAC-LC
//! through the operating system's encoder (macOS AudioToolbox). Recordings
//! are ADTS streams (`.aac`); uploads that need an MP4 container are `.m4a`.
//! Older MP3 recordings are still read (decode only, via Symphonia).

mod encoder;
mod error;
mod segment;
mod wav;

#[cfg(any(not(target_os = "macos"), feature = "flac-platform", test))]
mod flac;
#[cfg(target_os = "macos")]
#[cfg_attr(feature = "flac-platform", allow(dead_code))]
mod macos;

/// macOS: AAC through AudioToolbox, ADTS streams and M4A upload files.
#[cfg(all(target_os = "macos", not(feature = "flac-platform")))]
mod platform {
    pub(crate) use crate::macos::{AdtsEncoder as StreamCore, M4aWriter as UploadWriter};

    impl StreamCore {
        /// ADTS has no stream header, so continuing a file is a new stream.
        pub(crate) fn continuing(
            sample_rate: u32,
            channels: u32,
            _existing_samples: Option<u64>,
        ) -> Result<Self, crate::Error> {
            Self::new(sample_rate, channels)
        }
    }

    pub const RECORDING_EXTENSION: &str = "aac";
    pub const RECORDING_FILE: &str = "audio.aac";
    pub const UPLOAD_EXTENSION: &str = "m4a";
    /// ADTS has no stream header: appending is just writing more frames.
    pub(crate) const STREAM_HAS_HEADER: bool = false;
}

/// Windows and Linux: FLAC (`flacenc`, pure Rust) for streams and uploads.
#[cfg(any(not(target_os = "macos"), feature = "flac-platform"))]
mod platform {
    pub(crate) use crate::flac::{FlacEncoder as StreamCore, FlacFileWriter as UploadWriter};

    pub const RECORDING_EXTENSION: &str = "flac";
    pub const RECORDING_FILE: &str = "audio.flac";
    pub const UPLOAD_EXTENSION: &str = "flac";
    /// A FLAC stream starts with `fLaC` + STREAMINFO, written once.
    pub(crate) const STREAM_HAS_HEADER: bool = true;
}

pub use encoder::{Mono, MonoStreamEncoder, Stereo, StereoStreamEncoder, StreamEncoder};
pub use error::Error;
pub use segment::encode_mono_segments;
pub use wav::{concat_files, decode_to_wav, encode_wav, transcode_for_upload};

/// Extension of streamed recordings, chunk files and `audio.<ext>`: `aac`
/// (ADTS) on macOS, `flac` elsewhere.
pub use platform::RECORDING_EXTENSION;
/// A session's retained recording: `audio.aac` on macOS, `audio.flac`
/// elsewhere.
pub use platform::RECORDING_FILE;
/// Extension of files sent to speech-to-text providers: `m4a` on macOS,
/// `flac` elsewhere; every provider accepts both.
pub use platform::UPLOAD_EXTENSION;

/// Extensions of every encoded recording this app writes or once wrote, on
/// any platform (recordings sync between platforms): read them all.
pub const ENCODED_RECORDING_EXTENSIONS: [&str; 3] = ["aac", "flac", "mp3"];
/// Every retained-recording file name, this platform's first.
pub const RECORDING_FILES: [&str; 3] = if platform::STREAM_HAS_HEADER {
    ["audio.flac", "audio.aac", "audio.mp3"]
} else {
    ["audio.aac", "audio.flac", "audio.mp3"]
};
