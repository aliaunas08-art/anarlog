use std::fs::File;
use std::io::{BufReader, BufWriter, Seek, Write};
use std::path::Path;

use hound::SampleFormat;

use crate::Error;
use crate::encoder::{f32_to_i16, int_to_i16};
use crate::platform::{StreamCore, UploadWriter};

const CHUNK_FRAMES: usize = 4096;

pub fn concat_files(paths: &[&Path], output: &Path) -> Result<(), Error> {
    write_atomic(output, |writer| {
        for path in paths {
            let mut reader = BufReader::new(File::open(path)?);
            std::io::copy(&mut reader, writer)?;
        }
        Ok(())
    })
}

/// Encodes a WAV file: an upload file when `output` has the upload extension
/// (`.m4a` on macOS, `.flac` elsewhere), otherwise a recording stream
/// (`.aac` ADTS on macOS).
pub fn encode_wav(wav_path: &Path, output: &Path) -> Result<(), Error> {
    let mut reader = hound::WavReader::open(wav_path)?;
    let spec = reader.spec();
    if !matches!(spec.channels, 1 | 2) {
        return Err(Error::UnsupportedChannelCount(spec.channels));
    }

    if is_upload_file(output) {
        return write_upload_atomic(
            output,
            spec.sample_rate,
            u32::from(spec.channels),
            |writer| for_each_interleaved(&mut reader, spec, |chunk| writer.write(chunk)),
        );
    }

    write_atomic(output, |out| {
        let mut encoder = StreamCore::new(spec.sample_rate, u32::from(spec.channels))?;
        let mut encoded = Vec::new();
        for_each_interleaved(&mut reader, spec, |chunk| {
            encoded.clear();
            encoder.encode(chunk, &mut encoded)?;
            out.write_all(&encoded)?;
            Ok(())
        })?;
        encoded.clear();
        encoder.flush(&mut encoded)?;
        out.write_all(&encoded)?;
        Ok(())
    })
}

pub(crate) fn is_upload_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(crate::UPLOAD_EXTENSION))
}

/// Writes an upload file next to `output` and renames it into place, so a
/// failed encode never leaves a half-written file at `output`.
pub(crate) fn write_upload_atomic(
    output: &Path,
    sample_rate: u32,
    channels: u32,
    write: impl FnOnce(&mut UploadWriter) -> Result<(), Error>,
) -> Result<(), Error> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = tempfile::Builder::new()
        .suffix(&format!(".{}", crate::UPLOAD_EXTENSION))
        .tempfile_in(parent)?
        .into_temp_path();
    let mut writer = UploadWriter::create(&temporary, sample_rate, channels)?;
    write(&mut writer)?;
    writer.finish()?;
    temporary
        .persist(output)
        .map(|_| ())
        .map_err(|error| Error::Io(error.error))
}

/// Reads every sample as interleaved i16 chunks (stereo pads an odd tail).
fn for_each_interleaved<R: std::io::Read + Seek>(
    reader: &mut hound::WavReader<R>,
    spec: hound::WavSpec,
    mut chunk: impl FnMut(&[i16]) -> Result<(), Error>,
) -> Result<(), Error> {
    let bits = spec.bits_per_sample;
    let stereo = spec.channels == 2;
    let mut interleaved = Vec::with_capacity(CHUNK_FRAMES * 2);
    macro_rules! run {
        ($samples:expr, $convert:expr) => {
            if stereo {
                encode_stereo_samples($samples, $convert, |left, right| {
                    interleaved.clear();
                    for (l, r) in left.iter().zip(right) {
                        interleaved.push(*l);
                        interleaved.push(*r);
                    }
                    chunk(&interleaved)
                })
            } else {
                encode_mono_samples($samples, $convert, |mono| chunk(mono))
            }
        };
    }
    match spec.sample_format {
        SampleFormat::Float => {
            if bits != 32 {
                return Err(Error::UnsupportedFloatBitDepth(bits));
            }
            run!(reader.samples::<f32>(), f32_to_i16)
        }
        SampleFormat::Int => match bits {
            1..=8 => run!(reader.samples::<i8>(), |s| int_to_i16(s as i32, bits)),
            9..=16 => run!(reader.samples::<i16>(), |s| int_to_i16(s as i32, bits)),
            17..=32 => run!(reader.samples::<i32>(), |s| int_to_i16(s, bits)),
            other => Err(Error::UnsupportedIntBitDepth(other)),
        },
    }
}

fn write_atomic(
    output: &Path,
    write: impl FnOnce(&mut BufWriter<&mut File>) -> Result<(), Error>,
) -> Result<(), Error> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        write(&mut writer)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
    }

    temporary
        .persist(output)
        .map(|_| ())
        .map_err(|error| Error::Io(error.error))
}

/// Re-encodes any supported audio file (AAC stream, older MP3, WAV, …) as an
/// upload file (`.m4a` on macOS, `.flac` elsewhere) with the same sample rate
/// and channels (more than two are rejected). Used before upload: some
/// speech-to-text providers accept M4A but not raw AAC streams.
pub fn transcode_for_upload(source_path: &Path, output: &Path) -> Result<(), Error> {
    use anlg_audio_utils::Source;

    let source = anlg_audio_utils::source_from_path(source_path)?;
    let channels = u16::from(source.channels());
    if !matches!(channels, 1 | 2) {
        return Err(Error::UnsupportedChannelCount(channels));
    }
    let sample_rate = u32::from(source.sample_rate());
    write_upload_atomic(output, sample_rate, u32::from(channels), |writer| {
        let frame = usize::from(channels);
        let mut pcm = Vec::with_capacity(CHUNK_FRAMES * frame);
        for sample in source {
            pcm.push(f32_to_i16(sample));
            if pcm.len() >= CHUNK_FRAMES * frame {
                writer.write(&pcm)?;
                pcm.clear();
            }
        }
        // A stream ends on a whole frame; drop a stray partial one.
        pcm.truncate(pcm.len() - pcm.len() % frame);
        writer.write(&pcm)
    })
}

/// Decodes any supported audio file (AAC, M4A, older MP3 recordings, …) to
/// a 32-bit float WAV.
pub fn decode_to_wav(audio_path: &Path, wav_path: &Path) -> Result<(), Error> {
    use anlg_audio_utils::Source;

    let source = anlg_audio_utils::source_from_path(audio_path)?;
    let channels: u16 = source.channels().into();
    let sample_rate: u32 = source.sample_rate().into();

    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(wav_path, spec)?;
    for sample in source {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

fn encode_mono_samples<S, I, F, E>(
    samples: I,
    mut sample_to_i16: F,
    mut encode_chunk: E,
) -> Result<(), Error>
where
    I: Iterator<Item = Result<S, hound::Error>>,
    F: FnMut(S) -> i16,
    E: FnMut(&[i16]) -> Result<(), Error>,
{
    let mut pcm_i16 = Vec::with_capacity(CHUNK_FRAMES);
    for sample in samples {
        pcm_i16.push(sample_to_i16(sample?));
        if pcm_i16.len() < CHUNK_FRAMES {
            continue;
        }

        encode_chunk(&pcm_i16)?;
        pcm_i16.clear();
    }

    if !pcm_i16.is_empty() {
        encode_chunk(&pcm_i16)?;
    }

    Ok(())
}

fn encode_stereo_samples<S, I, F, E>(
    mut samples: I,
    mut sample_to_i16: F,
    mut encode_chunk: E,
) -> Result<(), Error>
where
    I: Iterator<Item = Result<S, hound::Error>>,
    F: FnMut(S) -> i16,
    E: FnMut(&[i16], &[i16]) -> Result<(), Error>,
{
    let mut left = Vec::with_capacity(CHUNK_FRAMES);
    let mut right = Vec::with_capacity(CHUNK_FRAMES);

    loop {
        let Some(left_sample) = samples.next() else {
            break;
        };
        left.push(sample_to_i16(left_sample?));

        match samples.next() {
            Some(right_sample) => right.push(sample_to_i16(right_sample?)),
            None => right.push(0i16),
        }

        if left.len() < CHUNK_FRAMES {
            continue;
        }

        encode_chunk(&left, &right)?;
        left.clear();
        right.clear();
    }

    if !left.is_empty() {
        encode_chunk(&left, &right)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn encode_mono_samples_flushes_partial_tail() -> Result<(), Error> {
        let samples = (0..(CHUNK_FRAMES + 1))
            .map(|n| Ok(n as i16))
            .collect::<Vec<_>>()
            .into_iter();
        let mut chunk_sizes = Vec::new();

        encode_mono_samples(
            samples,
            |sample| sample,
            |chunk| {
                chunk_sizes.push(chunk.len());
                Ok(())
            },
        )?;

        assert_eq!(chunk_sizes, vec![CHUNK_FRAMES, 1]);
        Ok(())
    }

    #[test]
    fn encode_stereo_samples_pads_missing_right_sample() -> Result<(), Error> {
        let samples = vec![Ok(10i16), Ok(20i16), Ok(30i16)].into_iter();
        let mut encoded = Vec::new();

        encode_stereo_samples(
            samples,
            |sample| sample,
            |left, right| {
                encoded.push((left.to_vec(), right.to_vec()));
                Ok(())
            },
        )?;

        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].0, vec![10, 30]);
        assert_eq!(encoded[0].1, vec![20, 0]);
        Ok(())
    }

    #[test]
    fn concat_files_joins_bytes_in_order() -> Result<(), Error> {
        let dir = tempdir()?;
        let first = dir.path().join("a.aac");
        let second = dir.path().join("b.aac");
        let output = dir.path().join("out.aac");

        std::fs::write(&first, [1u8, 2, 3])?;
        std::fs::write(&second, [4u8, 5, 6])?;

        concat_files(&[&first, &second], &output)?;

        assert_eq!(std::fs::read(output)?, vec![1, 2, 3, 4, 5, 6]);
        Ok(())
    }

    #[test]
    fn atomic_write_cleans_partial_temp_and_preserves_output() -> Result<(), Error> {
        let dir = tempdir()?;
        let output = dir.path().join("out.aac");
        std::fs::write(&output, b"existing")?;

        let error = write_atomic(&output, |writer| {
            writer.write_all(b"partial")?;
            Err(Error::UnsupportedChannelCount(9))
        })
        .expect_err("injected write should fail");

        assert!(matches!(error, Error::UnsupportedChannelCount(9)));
        assert_eq!(std::fs::read(&output)?, b"existing");
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 1);
        Ok(())
    }
}
