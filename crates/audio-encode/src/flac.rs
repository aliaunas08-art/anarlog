//! FLAC, the format on Windows and Linux, where there is no AAC encoder the
//! app may use: `flacenc` (Apache-2.0, pure Rust, no system codec).
//!
//! Recordings need a stream that can be written as it grows, appended to and
//! read after a crash, like the ADTS stream on macOS. Frames are numbered in
//! FLAC's fixed-blocksize mode and every stream ends on a whole frame (the
//! tail is padded with at most 64 ms of silence at 16 kHz), so a stream that
//! continues an existing file starts at the next frame number and skips the
//! header. The header (`fLaC` + STREAMINFO, length and MD5 left unknown,
//! which FLAC allows) is written once, at the start of a stream.

use std::path::Path;

use flacenc::bitsink::ByteSink;
use flacenc::component::{BitRepr, Stream, StreamInfo};
use flacenc::config;
use flacenc::error::Verify;
use flacenc::source::{Fill, FrameBuf};

use crate::Error;

/// Samples per channel in every frame.
const BLOCK_SIZE: usize = 1024;
const BITS_PER_SAMPLE: usize = 16;

fn flac_error(error: impl std::fmt::Display) -> Error {
    Error::Flac(error.to_string())
}

/// Streaming PCM (interleaved i16) → FLAC frames.
pub(crate) struct FlacEncoder {
    config: flacenc::error::Verified<config::Encoder>,
    stream_info: StreamInfo,
    channels: usize,
    framebuf: FrameBuf,
    pending: Vec<i32>,
    next_frame: usize,
    header_pending: bool,
    /// Frames written since the stream started (a header with no frames
    /// doesn't decode, so an empty stream ends with one silent frame).
    frames_written: usize,
}

impl FlacEncoder {
    /// A new stream: its first output starts with the stream header.
    pub(crate) fn new(sample_rate: u32, channels: u32) -> Result<Self, Error> {
        Self::continuing(sample_rate, channels, None)
    }

    /// Continues a stream that already holds `existing_samples` per channel
    /// (whole frames, as this encoder writes them): no header, numbering
    /// continues. `None` starts a new stream.
    pub(crate) fn continuing(
        sample_rate: u32,
        channels: u32,
        existing_samples: Option<u64>,
    ) -> Result<Self, Error> {
        let channels = channels as usize;
        let mut stream_info =
            StreamInfo::new(sample_rate as usize, channels, BITS_PER_SAMPLE).map_err(flac_error)?;
        stream_info
            .set_block_sizes(BLOCK_SIZE, BLOCK_SIZE)
            .map_err(flac_error)?;
        let config = config::Encoder::default()
            .into_verified()
            .map_err(|(_, error)| flac_error(error))?;
        Ok(Self {
            config,
            stream_info,
            channels,
            framebuf: FrameBuf::with_size(channels, BLOCK_SIZE).map_err(flac_error)?,
            pending: Vec::new(),
            next_frame: existing_samples.map_or(0, |samples| samples as usize / BLOCK_SIZE),
            header_pending: existing_samples.is_none(),
            frames_written: 0,
        })
    }

    fn write_header(&mut self, output: &mut Vec<u8>) -> Result<(), Error> {
        if !self.header_pending {
            return Ok(());
        }
        let stream = Stream::with_stream_info(self.stream_info.clone());
        let mut sink = ByteSink::new();
        stream.write(&mut sink).map_err(flac_error)?;
        output.extend_from_slice(sink.as_slice());
        self.header_pending = false;
        Ok(())
    }

    /// Encodes one frame from the first `frames` pending frames, padding a
    /// short tail with silence to a whole frame.
    fn encode_block(&mut self, frames: usize, output: &mut Vec<u8>) -> Result<(), Error> {
        let samples = frames * self.channels;
        let mut block: Vec<i32> = self.pending[..samples].to_vec();
        block.resize(BLOCK_SIZE * self.channels, 0);
        self.framebuf.fill_interleaved(&block).map_err(flac_error)?;
        let frame = flacenc::encode_fixed_size_frame(
            &self.config,
            &self.framebuf,
            self.next_frame,
            &self.stream_info,
        )
        .map_err(flac_error)?;
        let mut sink = ByteSink::new();
        frame.write(&mut sink).map_err(flac_error)?;
        output.extend_from_slice(sink.as_slice());
        self.pending.drain(..samples);
        self.next_frame += 1;
        self.frames_written += 1;
        Ok(())
    }

    pub(crate) fn encode(
        &mut self,
        interleaved: &[i16],
        output: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.write_header(output)?;
        self.pending
            .extend(interleaved.iter().map(|sample| i32::from(*sample)));
        while self.pending.len() >= BLOCK_SIZE * self.channels {
            self.encode_block(BLOCK_SIZE, output)?;
        }
        Ok(())
    }

    /// Encodes what is left (the last frame may be short) and starts a new
    /// stream for reuse.
    pub(crate) fn flush(&mut self, output: &mut Vec<u8>) -> Result<(), Error> {
        self.write_header(output)?;
        let frames = self.pending.len() / self.channels;
        if frames > 0 || self.frames_written == 0 {
            self.encode_block(frames, output)?;
        }
        self.pending.clear();
        self.next_frame = 0;
        self.header_pending = true;
        self.frames_written = 0;
        Ok(())
    }
}

/// Writes PCM (interleaved i16) into a `.flac` file.
pub(crate) struct FlacFileWriter {
    file: std::io::BufWriter<std::fs::File>,
    encoder: FlacEncoder,
    encoded: Vec<u8>,
}

impl FlacFileWriter {
    pub(crate) fn create(path: &Path, sample_rate: u32, channels: u32) -> Result<Self, Error> {
        Ok(Self {
            file: std::io::BufWriter::new(std::fs::File::create(path)?),
            encoder: FlacEncoder::new(sample_rate, channels)?,
            encoded: Vec::new(),
        })
    }

    pub(crate) fn write(&mut self, interleaved: &[i16]) -> Result<(), Error> {
        use std::io::Write;
        self.encoded.clear();
        self.encoder.encode(interleaved, &mut self.encoded)?;
        self.file.write_all(&self.encoded)?;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<(), Error> {
        use std::io::Write;
        self.encoded.clear();
        self.encoder.flush(&mut self.encoded)?;
        self.file.write_all(&self.encoded)?;
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, channels: usize) -> Vec<i16> {
        (0..frames * channels)
            .map(|i| (((i / channels) as f32 * 0.07).sin() * 9000.0) as i16)
            .collect()
    }

    fn decode(bytes: &[u8]) -> (u16, u32, usize) {
        use anlg_audio_utils::Source;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.flac");
        std::fs::write(&path, bytes).unwrap();
        let source = anlg_audio_utils::source_from_path(&path).unwrap();
        let channels = u16::from(source.channels());
        let rate = u32::from(source.sample_rate());
        (channels, rate, source.count() / channels as usize)
    }

    #[test]
    fn a_stream_decodes_to_its_length() {
        let mut encoder = FlacEncoder::new(16_000, 2).unwrap();
        let mut bytes = Vec::new();
        encoder.encode(&tone(10_000, 2), &mut bytes).unwrap();
        encoder.encode(&tone(7_001, 2), &mut bytes).unwrap();
        encoder.flush(&mut bytes).unwrap();
        assert_eq!(&bytes[..4], b"fLaC");
        let (channels, rate, frames) = decode(&bytes);
        assert_eq!((channels, rate), (2, 16_000));
        // The tail is padded to a whole frame.
        assert_eq!(frames, 17_001_usize.div_ceil(BLOCK_SIZE) * BLOCK_SIZE);
    }

    #[test]
    fn a_continued_stream_appends_to_an_existing_file() {
        let mut first = FlacEncoder::new(16_000, 1).unwrap();
        let mut bytes = Vec::new();
        first.encode(&tone(8_192, 1), &mut bytes).unwrap();
        first.flush(&mut bytes).unwrap();

        let mut next = FlacEncoder::continuing(16_000, 1, Some(8_192)).unwrap();
        let mut appended = Vec::new();
        next.encode(&tone(4_096, 1), &mut appended).unwrap();
        next.flush(&mut appended).unwrap();
        assert_ne!(&appended[..4], b"fLaC", "no second header");
        bytes.extend_from_slice(&appended);

        let (_, _, frames) = decode(&bytes);
        assert_eq!(frames, 8_192 + 4_096);
    }

    #[test]
    fn a_crash_leaves_a_readable_prefix() {
        let mut encoder = FlacEncoder::new(16_000, 2).unwrap();
        let mut bytes = Vec::new();
        encoder
            .encode(&tone(BLOCK_SIZE * 3, 2), &mut bytes)
            .unwrap();
        // No flush: the process died mid-recording.
        let (_, _, frames) = decode(&bytes);
        assert_eq!(frames, BLOCK_SIZE * 3);
    }

    #[test]
    fn a_flac_file_is_written_and_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upload.flac");
        let mut writer = FlacFileWriter::create(&path, 16_000, 1).unwrap();
        writer.write(&tone(5_000, 1)).unwrap();
        writer.finish().unwrap();
        let (channels, rate, frames) = decode(&std::fs::read(&path).unwrap());
        assert_eq!((channels, rate), (1, 16_000));
        assert_eq!(frames, 5_000_usize.div_ceil(BLOCK_SIZE) * BLOCK_SIZE);
    }
}
