//! AAC-LC through macOS AudioToolbox: a streaming encoder that emits ADTS
//! frames (appendable, readable after a crash, concatenable like MP3), and an
//! `.m4a` file writer for providers that want an MP4 container.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;

use objc2_audio_toolbox::{
    AudioConverterComplexInputDataProc, AudioConverterDispose, AudioConverterFillComplexBuffer,
    AudioConverterGetProperty, AudioConverterGetPropertyInfo, AudioConverterNew, AudioConverterRef,
    AudioConverterSetProperty, ExtAudioFileCreateWithURL, ExtAudioFileDispose,
    ExtAudioFileGetProperty, ExtAudioFileRef, ExtAudioFileSetProperty, ExtAudioFileWrite,
    kAudioConverterApplicableEncodeBitRates, kAudioConverterEncodeBitRate,
    kAudioConverterPropertyMaximumOutputPacketSize, kAudioFileM4AType,
    kExtAudioFileProperty_AudioConverter, kExtAudioFileProperty_ClientDataFormat,
    kExtAudioFileProperty_ConverterConfig,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioStreamPacketDescription,
    AudioValueRange, kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM, kAudioFormatMPEG4AAC,
};
use objc2_core_foundation::{CFString, CFURL, CFURLPathStyle};

use crate::Error;

/// AAC-LC frames are always 1024 PCM frames.
const AAC_FRAMES_PER_PACKET: u32 = 1024;
/// MPEG-4 audio object type for AAC-LC.
const AAC_LC_OBJECT_TYPE: u32 = 2;
/// Returned by the input callback when it has no more PCM for now; not an
/// AudioToolbox error, just "call again with more input".
const NEED_MORE_INPUT: i32 = 0x6d6f7265; // 'more'
/// `kAudioFileFlags_EraseFile`.
const ERASE_FILE: u32 = 1;
/// Packets requested per FillComplexBuffer call.
const PACKETS_PER_FILL: u32 = 16;

fn check(status: i32, what: &'static str) -> Result<(), Error> {
    if status == 0 {
        Ok(())
    } else {
        Err(Error::AudioToolbox { what, status })
    }
}

fn pcm_format(sample_rate: u32, channels: u32) -> AudioStreamBasicDescription {
    let bytes_per_frame = 2 * channels;
    AudioStreamBasicDescription {
        mSampleRate: f64::from(sample_rate),
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
        mBytesPerPacket: bytes_per_frame,
        mFramesPerPacket: 1,
        mBytesPerFrame: bytes_per_frame,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 16,
        mReserved: 0,
    }
}

fn aac_format(sample_rate: u32, channels: u32) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: f64::from(sample_rate),
        mFormatID: kAudioFormatMPEG4AAC,
        mFormatFlags: AAC_LC_OBJECT_TYPE,
        mBytesPerPacket: 0,
        mFramesPerPacket: AAC_FRAMES_PER_PACKET,
        mBytesPerFrame: 0,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 0,
        mReserved: 0,
    }
}

/// Bitrate the old MP3 recordings used (64 kbps mono, 128 kbps stereo),
/// lowered to the highest rate the encoder allows at this sample rate and
/// channel count; without one, the encoder's default stays.
fn target_bitrate(channels: u32) -> f64 {
    if channels == 1 { 64_000.0 } else { 128_000.0 }
}

fn applicable_bitrates(converter: AudioConverterRef) -> Vec<AudioValueRange> {
    let mut size: u32 = 0;
    // SAFETY: valid converter; out pointers are valid.
    let status = unsafe {
        AudioConverterGetPropertyInfo(
            converter,
            kAudioConverterApplicableEncodeBitRates,
            &mut size,
            std::ptr::null_mut(),
        )
    };
    let count = size as usize / std::mem::size_of::<AudioValueRange>();
    if status != 0 || count == 0 {
        return Vec::new();
    }
    let mut ranges = vec![
        AudioValueRange {
            mMinimum: 0.0,
            mMaximum: 0.0,
        };
        count
    ];
    // SAFETY: the buffer holds `count` ranges, `size` bytes.
    let status = unsafe {
        AudioConverterGetProperty(
            converter,
            kAudioConverterApplicableEncodeBitRates,
            NonNull::from(&mut size),
            NonNull::new(ranges.as_mut_ptr().cast()).expect("non-null vec"),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    ranges.truncate(size as usize / std::mem::size_of::<AudioValueRange>());
    ranges
}

/// The highest allowed bitrate at or below `target`, else the lowest allowed.
pub(crate) fn choose_bitrate(target: f64, ranges: &[AudioValueRange]) -> Option<u32> {
    let at_or_below = ranges
        .iter()
        .filter(|range| range.mMinimum <= target)
        .map(|range| range.mMaximum.min(target))
        .fold(None, |best: Option<f64>, rate| {
            Some(best.map_or(rate, |b| b.max(rate)))
        });
    at_or_below
        .or_else(|| {
            ranges
                .iter()
                .map(|range| range.mMinimum)
                .fold(None, |best: Option<f64>, rate| {
                    Some(best.map_or(rate, |b| b.min(rate)))
                })
        })
        .filter(|rate| *rate > 0.0)
        .map(|rate| rate as u32)
}

fn set_bitrate(converter: AudioConverterRef, channels: u32) {
    let Some(mut bitrate) =
        choose_bitrate(target_bitrate(channels), &applicable_bitrates(converter))
    else {
        return;
    };
    // SAFETY: valid converter; the property is a u32.
    let _ = unsafe {
        AudioConverterSetProperty(
            converter,
            kAudioConverterEncodeBitRate,
            std::mem::size_of::<u32>() as u32,
            NonNull::from(&mut bitrate).cast(),
        )
    };
}

/// ADTS sampling-frequency index (ISO/IEC 14496-3).
fn adts_frequency_index(sample_rate: u32) -> Result<u8, Error> {
    const RATES: [u32; 13] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000, 7_350,
    ];
    RATES
        .iter()
        .position(|rate| *rate == sample_rate)
        .map(|index| index as u8)
        .ok_or(Error::UnsupportedSampleRate(sample_rate))
}

/// The 7-byte ADTS header (no CRC) for one AAC-LC packet.
pub(crate) fn adts_header(frequency_index: u8, channels: u8, payload_len: usize) -> [u8; 7] {
    let frame_len = payload_len + 7;
    let profile = (AAC_LC_OBJECT_TYPE - 1) as u8;
    [
        0xFF,
        0xF1, // MPEG-4, layer 0, no CRC
        (profile << 6) | (frequency_index << 2) | ((channels >> 2) & 0x1),
        ((channels & 0x3) << 6) | ((frame_len >> 11) as u8 & 0x3),
        ((frame_len >> 3) & 0xFF) as u8,
        (((frame_len & 0x7) << 5) as u8) | 0x1F,
        0xFC,
    ]
}

/// PCM handed to the converter's input callback.
struct Feed<'a> {
    pcm: &'a [i16],
    offset: usize,
    channels: u32,
    end_of_stream: bool,
}

unsafe extern "C-unwind" fn feed_pcm(
    _converter: AudioConverterRef,
    io_packets: NonNull<u32>,
    io_data: NonNull<AudioBufferList>,
    _packet_descriptions: *mut *mut AudioStreamPacketDescription,
    user_data: *mut c_void,
) -> i32 {
    // SAFETY: user_data is the `Feed` passed to FillComplexBuffer, alive for
    // the whole call; io_* are valid for the callback's duration.
    unsafe {
        let feed = &mut *(user_data as *mut Feed<'_>);
        let channels = feed.channels as usize;
        let available = (feed.pcm.len() - feed.offset) / channels;
        let list = &mut *io_data.as_ptr();
        if available == 0 {
            *io_packets.as_ptr() = 0;
            list.mBuffers[0].mDataByteSize = 0;
            list.mBuffers[0].mData = std::ptr::null_mut();
            return if feed.end_of_stream {
                0
            } else {
                NEED_MORE_INPUT
            };
        }
        let frames = available.min(*io_packets.as_ptr() as usize);
        let start = feed.pcm.as_ptr().add(feed.offset);
        list.mBuffers[0].mNumberChannels = feed.channels;
        list.mBuffers[0].mDataByteSize = (frames * channels * 2) as u32;
        list.mBuffers[0].mData = start as *mut c_void;
        *io_packets.as_ptr() = frames as u32;
        feed.offset += frames * channels;
        0
    }
}

/// Streaming PCM (interleaved i16) → ADTS AAC-LC.
pub(crate) struct AdtsEncoder {
    converter: AudioConverterRef,
    channels: u32,
    frequency_index: u8,
    max_packet_size: u32,
    pending: Vec<i16>,
}

// SAFETY: the converter is only used through `&mut self`.
unsafe impl Send for AdtsEncoder {}

impl AdtsEncoder {
    pub(crate) fn new(sample_rate: u32, channels: u32) -> Result<Self, Error> {
        let frequency_index = adts_frequency_index(sample_rate)?;
        let input = pcm_format(sample_rate, channels);
        let output = aac_format(sample_rate, channels);
        let mut converter: AudioConverterRef = std::ptr::null_mut();
        // SAFETY: valid descriptions and out pointer.
        check(
            unsafe {
                AudioConverterNew(
                    NonNull::from(&input),
                    NonNull::from(&output),
                    NonNull::from(&mut converter),
                )
            },
            "AudioConverterNew",
        )?;
        set_bitrate(converter, channels);
        let mut max_packet_size: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: valid converter; the property is a u32.
        let status = unsafe {
            AudioConverterGetProperty(
                converter,
                kAudioConverterPropertyMaximumOutputPacketSize,
                NonNull::from(&mut size),
                NonNull::from(&mut max_packet_size).cast(),
            )
        };
        if status != 0 || max_packet_size == 0 {
            // SAFETY: created above.
            unsafe { AudioConverterDispose(converter) };
            return Err(Error::AudioToolbox {
                what: "MaximumOutputPacketSize",
                status,
            });
        }
        Ok(Self {
            converter,
            channels,
            frequency_index,
            max_packet_size,
            pending: Vec::new(),
        })
    }

    /// Queues interleaved PCM and writes every complete AAC frame to `output`.
    pub(crate) fn encode(
        &mut self,
        interleaved: &[i16],
        output: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.pending.extend_from_slice(interleaved);
        self.drain(false, output)
    }

    /// Encodes what is left (padding the last frame) and resets for reuse.
    pub(crate) fn flush(&mut self, output: &mut Vec<u8>) -> Result<(), Error> {
        self.drain(true, output)?;
        self.pending.clear();
        Ok(())
    }

    fn drain(&mut self, end_of_stream: bool, output: &mut Vec<u8>) -> Result<(), Error> {
        let packet_capacity = self.max_packet_size as usize * PACKETS_PER_FILL as usize;
        let mut packet_bytes = vec![0u8; packet_capacity];
        let mut descriptions = vec![
            AudioStreamPacketDescription {
                mStartOffset: 0,
                mVariableFramesInPacket: 0,
                mDataByteSize: 0,
            };
            PACKETS_PER_FILL as usize
        ];
        let frames_per_fill = (AAC_FRAMES_PER_PACKET * PACKETS_PER_FILL) as usize;
        loop {
            let available_frames = self.pending.len() / self.channels as usize;
            if !end_of_stream && available_frames < frames_per_fill {
                // Feed whole fills only while streaming; the converter keeps
                // its own look-ahead, so smaller writes simply wait.
                if available_frames < AAC_FRAMES_PER_PACKET as usize {
                    return Ok(());
                }
            }
            let mut feed = Feed {
                pcm: &self.pending,
                offset: 0,
                channels: self.channels,
                end_of_stream,
            };
            let mut list = AudioBufferList {
                mNumberBuffers: 1,
                mBuffers: [AudioBuffer {
                    mNumberChannels: self.channels,
                    mDataByteSize: packet_capacity as u32,
                    mData: packet_bytes.as_mut_ptr() as *mut c_void,
                }],
            };
            let mut packets = PACKETS_PER_FILL;
            let proc: AudioConverterComplexInputDataProc = Some(feed_pcm);
            // SAFETY: `feed`, `list`, `packet_bytes` and `descriptions` outlive
            // the call; the callback only reads `feed.pcm`.
            let status = unsafe {
                AudioConverterFillComplexBuffer(
                    self.converter,
                    proc,
                    (&mut feed as *mut Feed<'_>).cast(),
                    NonNull::from(&mut packets),
                    NonNull::from(&mut list),
                    descriptions.as_mut_ptr(),
                )
            };
            let consumed = feed.offset;
            if status != 0 && status != NEED_MORE_INPUT {
                return Err(Error::AudioToolbox {
                    what: "AudioConverterFillComplexBuffer",
                    status,
                });
            }
            for description in &descriptions[..packets as usize] {
                let start = description.mStartOffset as usize;
                let len = description.mDataByteSize as usize;
                output.extend_from_slice(&adts_header(
                    self.frequency_index,
                    self.channels as u8,
                    len,
                ));
                output.extend_from_slice(&packet_bytes[start..start + len]);
            }
            self.pending.drain(..consumed);
            // Stop when the converter wants more input than we have, or when
            // the end of the stream has been fully flushed out.
            if status == NEED_MORE_INPUT || packets == 0 {
                if end_of_stream {
                    // SAFETY: valid converter; ready it for another stream.
                    unsafe { objc2_audio_toolbox::AudioConverterReset(self.converter) };
                }
                return Ok(());
            }
        }
    }
}

impl Drop for AdtsEncoder {
    fn drop(&mut self) {
        // SAFETY: created in `new`, disposed once.
        unsafe { AudioConverterDispose(self.converter) };
    }
}

/// Writes PCM (interleaved i16) into an `.m4a` (AAC-LC in MP4) file.
pub(crate) struct M4aWriter {
    file: ExtAudioFileRef,
    channels: u32,
}

// SAFETY: the file is only used through `&mut self`.
unsafe impl Send for M4aWriter {}

impl M4aWriter {
    pub(crate) fn create(path: &Path, sample_rate: u32, channels: u32) -> Result<Self, Error> {
        adts_frequency_index(sample_rate)?;
        let path_text = path.to_str().ok_or(Error::NonUtf8Path)?;
        let cf_path = CFString::from_str(path_text);
        let url = CFURL::with_file_system_path(
            None,
            Some(&cf_path),
            CFURLPathStyle::CFURLPOSIXPathStyle,
            false,
        )
        .ok_or(Error::NonUtf8Path)?;
        let file_format = aac_format(sample_rate, channels);
        let mut file: ExtAudioFileRef = std::ptr::null_mut();
        // SAFETY: valid URL, description and out pointer.
        check(
            unsafe {
                ExtAudioFileCreateWithURL(
                    &url,
                    kAudioFileM4AType,
                    NonNull::from(&file_format),
                    std::ptr::null(),
                    ERASE_FILE,
                    NonNull::from(&mut file),
                )
            },
            "ExtAudioFileCreateWithURL",
        )?;
        let writer = Self { file, channels };
        let client = pcm_format(sample_rate, channels);
        // SAFETY: valid file; the property is an AudioStreamBasicDescription.
        check(
            unsafe {
                ExtAudioFileSetProperty(
                    writer.file,
                    kExtAudioFileProperty_ClientDataFormat,
                    std::mem::size_of::<AudioStreamBasicDescription>() as u32,
                    NonNull::from(&client).cast(),
                )
            },
            "ClientDataFormat",
        )?;
        writer.set_bitrate();
        Ok(writer)
    }

    fn set_bitrate(&self) {
        let mut converter: AudioConverterRef = std::ptr::null_mut();
        let mut size = std::mem::size_of::<AudioConverterRef>() as u32;
        // SAFETY: valid file; the property is an AudioConverterRef it owns.
        let status = unsafe {
            ExtAudioFileGetProperty(
                self.file,
                kExtAudioFileProperty_AudioConverter,
                NonNull::from(&mut size),
                NonNull::from(&mut converter).cast(),
            )
        };
        if status != 0 || converter.is_null() {
            return;
        }
        set_bitrate(converter, self.channels);
        // Tell ExtAudioFile the converter's settings changed.
        let mut no_config: *const c_void = std::ptr::null();
        // SAFETY: a null CFPropertyList pointer is the documented way to
        // apply the converter's current settings.
        let _ = unsafe {
            ExtAudioFileSetProperty(
                self.file,
                kExtAudioFileProperty_ConverterConfig,
                std::mem::size_of::<*const c_void>() as u32,
                NonNull::from(&mut no_config).cast(),
            )
        };
    }

    pub(crate) fn write(&mut self, interleaved: &[i16]) -> Result<(), Error> {
        if interleaved.is_empty() {
            return Ok(());
        }
        let frames = interleaved.len() / self.channels as usize;
        let mut list = AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: self.channels,
                mDataByteSize: (frames * self.channels as usize * 2) as u32,
                mData: interleaved.as_ptr() as *mut c_void,
            }],
        };
        // SAFETY: the buffer list points into `interleaved` for the call.
        check(
            unsafe { ExtAudioFileWrite(self.file, frames as u32, NonNull::from(&mut list)) },
            "ExtAudioFileWrite",
        )
    }

    /// Finishes the file (writes the MP4 index).
    pub(crate) fn finish(mut self) -> Result<(), Error> {
        let file = std::mem::replace(&mut self.file, std::ptr::null_mut());
        // SAFETY: created in `create`, disposed once.
        check(unsafe { ExtAudioFileDispose(file) }, "ExtAudioFileDispose")
    }
}

impl Drop for M4aWriter {
    fn drop(&mut self) {
        if !self.file.is_null() {
            // SAFETY: created in `create`, not yet disposed.
            unsafe { ExtAudioFileDispose(self.file) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adts_header_encodes_profile_rate_channels_and_length() {
        let header = adts_header(8, 2, 100);
        assert_eq!(&header[..2], &[0xFF, 0xF1]);
        // profile LC (1), 16 kHz (index 8), channel config 2
        assert_eq!(header[2] >> 6, 1);
        assert_eq!((header[2] >> 2) & 0xF, 8);
        assert_eq!(((header[2] & 1) << 2) | (header[3] >> 6), 2);
        let frame_len = (usize::from(header[3] & 0x3) << 11)
            | (usize::from(header[4]) << 3)
            | (usize::from(header[5]) >> 5);
        assert_eq!(frame_len, 107);
    }

    #[test]
    fn bitrate_is_clamped_to_what_the_encoder_allows() {
        let ranges = [
            AudioValueRange {
                mMinimum: 8_000.0,
                mMaximum: 8_000.0,
            },
            AudioValueRange {
                mMinimum: 24_000.0,
                mMaximum: 24_000.0,
            },
            AudioValueRange {
                mMinimum: 40_000.0,
                mMaximum: 40_000.0,
            },
        ];
        assert_eq!(choose_bitrate(64_000.0, &ranges), Some(40_000));
        assert_eq!(choose_bitrate(24_000.0, &ranges), Some(24_000));
        assert_eq!(choose_bitrate(4_000.0, &ranges), Some(8_000));
        assert_eq!(choose_bitrate(64_000.0, &[]), None);
    }

    #[test]
    fn rejects_rates_adts_cannot_signal() {
        assert!(adts_frequency_index(16_000).is_ok());
        assert!(matches!(
            adts_frequency_index(15_000),
            Err(Error::UnsupportedSampleRate(15_000))
        ));
    }
}
