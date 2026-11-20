use std::marker::PhantomData;

use crate::Error;
use crate::platform::StreamCore;

pub struct Mono;
pub struct Stereo;

pub type MonoStreamEncoder = StreamEncoder<Mono>;
pub type StereoStreamEncoder = StreamEncoder<Stereo>;

/// Streaming PCM → AAC-LC in ADTS frames (`.aac`) on macOS, FLAC frames
/// (`.flac`) elsewhere. Each `encode_*` call appends whole frames to
/// `output`; the stream can be written to a file as it grows, appended to and
/// read back after a crash, like the MP3 stream it replaced.
pub struct StreamEncoder<M> {
    core: StreamCore,
    _marker: PhantomData<M>,
}

impl<M> StreamEncoder<M> {
    fn with_channels(
        sample_rate: u32,
        channels: u32,
        existing_samples: Option<u64>,
    ) -> Result<Self, Error> {
        Ok(Self {
            core: StreamCore::continuing(sample_rate, channels, existing_samples)?,
            _marker: PhantomData,
        })
    }

    pub fn flush(&mut self, output: &mut Vec<u8>) -> Result<(), Error> {
        self.core.flush(output)
    }
}

impl StreamEncoder<Mono> {
    pub fn new(sample_rate: u32) -> Result<Self, Error> {
        Self::with_channels(sample_rate, 1, None)
    }

    pub fn encode_f32(&mut self, samples: &[f32], output: &mut Vec<u8>) -> Result<(), Error> {
        if samples.is_empty() {
            return Ok(());
        }
        let pcm: Vec<i16> = samples.iter().copied().map(f32_to_i16).collect();
        self.encode_i16(&pcm, output)
    }

    pub fn encode_i16(&mut self, samples: &[i16], output: &mut Vec<u8>) -> Result<(), Error> {
        if samples.is_empty() {
            return Ok(());
        }
        self.core.encode(samples, output)
    }
}

impl StreamEncoder<Stereo> {
    pub fn new(sample_rate: u32) -> Result<Self, Error> {
        Self::with_channels(sample_rate, 2, None)
    }

    /// Continues an existing recording file that holds `existing_samples`
    /// per channel: FLAC then writes no second header and keeps numbering
    /// frames; for ADTS this is the same as `new`.
    pub fn continuing(sample_rate: u32, existing_samples: u64) -> Result<Self, Error> {
        Self::with_channels(sample_rate, 2, Some(existing_samples))
    }

    /// For appending to the recording at `path`: continues the stream it
    /// holds (FLAC needs the sample count to keep numbering frames and must
    /// not repeat its header), or starts a new one when there is none.
    pub fn appending_to(sample_rate: u32, path: &std::path::Path) -> Result<Self, Error> {
        let existing_len = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if existing_len == 0 || !crate::platform::STREAM_HAS_HEADER {
            return Self::new(sample_rate);
        }
        use anlg_audio_utils::Source;
        let source = anlg_audio_utils::source_from_path(path)?;
        let channels = usize::from(u16::from(source.channels())).max(1);
        let samples = (source.count() / channels) as u64;
        Self::continuing(sample_rate, samples)
    }

    pub fn encode_f32(
        &mut self,
        left: &[f32],
        right: &[f32],
        output: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let frames = left.len().max(right.len());
        if frames == 0 {
            return Ok(());
        }
        let mut interleaved = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            interleaved.push(f32_to_i16(left.get(i).copied().unwrap_or(0.0)));
            interleaved.push(f32_to_i16(right.get(i).copied().unwrap_or(0.0)));
        }
        self.core.encode(&interleaved, output)
    }

    pub fn encode_i16(
        &mut self,
        left: &[i16],
        right: &[i16],
        output: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let frames = left.len().max(right.len());
        if frames == 0 {
            return Ok(());
        }
        let mut interleaved = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            interleaved.push(left.get(i).copied().unwrap_or(0));
            interleaved.push(right.get(i).copied().unwrap_or(0));
        }
        self.core.encode(&interleaved, output)
    }
}

pub(crate) use anlg_audio_utils::f32_to_i16;

pub(crate) fn int_to_i16(sample: i32, bits_per_sample: u16) -> i16 {
    let max_amplitude = match bits_per_sample {
        0 | 1 => return 0,
        32.. => i32::MAX as f32,
        bits => ((1i64 << (bits - 1)) - 1) as f32,
    };
    f32_to_i16(sample as f32 / max_amplitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_conversion_clamps_and_scales() {
        assert_eq!(f32_to_i16(-2.0), -i16::MAX);
        assert_eq!(f32_to_i16(2.0), i16::MAX);
        assert_eq!(int_to_i16(i32::MAX, 32), i16::MAX);
        assert_eq!(int_to_i16(i32::MIN, 32), -i16::MAX);
        assert_eq!(int_to_i16(1, 1), 0);
    }
}
