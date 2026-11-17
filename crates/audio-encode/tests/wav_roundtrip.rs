mod common;

use audio_encode::{decode_to_wav, encode_wav};
use tempfile::tempdir;

use crate::common::{
    Case, TestResult, assert_decoded_matches_case, assert_samples_valid, assert_upload_container,
    read_wav, write_fixture_wav, write_fixture_wav_int,
};

fn assert_float_wav_roundtrip(case: Case) -> TestResult {
    let tempdir = tempdir()?;
    let wav_path = tempdir.path().join("input.wav");
    let aac_path = tempdir
        .path()
        .join(format!("encoded.{}", audio_encode::RECORDING_EXTENSION));
    let decoded_wav_path = tempdir.path().join("decoded.wav");

    let original_samples = write_fixture_wav(&wav_path, case)?;
    encode_wav(&wav_path, &aac_path)?;
    decode_to_wav(&aac_path, &decoded_wav_path)?;

    assert!(aac_path.exists(), "encoded aac was not created");
    let encoded_size = std::fs::metadata(&aac_path)?.len();
    assert!(encoded_size > 0, "encoded aac is empty");

    let (decoded_spec, decoded_samples) = read_wav(&decoded_wav_path)?;
    assert_decoded_matches_case(
        case,
        original_samples.len(),
        &decoded_spec,
        &decoded_samples,
    );
    Ok(())
}

fn assert_pcm_wav_roundtrip(case: Case, bits_per_sample: u16) -> TestResult {
    let tempdir = tempdir()?;
    let wav_path = tempdir.path().join("input.wav");
    let aac_path = tempdir
        .path()
        .join(format!("encoded.{}", audio_encode::RECORDING_EXTENSION));
    let decoded_wav_path = tempdir.path().join("decoded.wav");

    write_fixture_wav_int(&wav_path, case, bits_per_sample)?;
    encode_wav(&wav_path, &aac_path)?;
    decode_to_wav(&aac_path, &decoded_wav_path)?;

    let (decoded_spec, decoded_samples) = read_wav(&decoded_wav_path)?;
    assert_eq!(
        decoded_spec.channels, case.channels,
        "channel count changed"
    );
    assert_eq!(
        decoded_spec.sample_rate, case.sample_rate,
        "sample rate changed"
    );
    assert!(!decoded_samples.is_empty(), "decoded output is empty");
    assert_samples_valid(&decoded_samples);
    Ok(())
}

#[test]
fn float_wav_roundtrips_across_chunk_boundaries() {
    let cases = [
        Case {
            channels: 1,
            frames: 0,
            sample_rate: 16_000,
        },
        Case {
            channels: 1,
            frames: 1,
            sample_rate: 16_000,
        },
        Case {
            channels: 1,
            frames: 4_096,
            sample_rate: 16_000,
        },
        Case {
            channels: 1,
            frames: 4_097,
            sample_rate: 16_000,
        },
        Case {
            channels: 1,
            frames: 12_345,
            sample_rate: 16_000,
        },
        Case {
            channels: 2,
            frames: 0,
            sample_rate: 48_000,
        },
        Case {
            channels: 2,
            frames: 1,
            sample_rate: 48_000,
        },
        Case {
            channels: 2,
            frames: 4_096,
            sample_rate: 48_000,
        },
        Case {
            channels: 2,
            frames: 4_097,
            sample_rate: 48_000,
        },
        Case {
            channels: 2,
            frames: 11_111,
            sample_rate: 48_000,
        },
    ];

    for case in cases {
        assert_float_wav_roundtrip(case).unwrap_or_else(|error| panic!("{case:?}: {error}"));
    }
}

#[test]
fn pcm_wav_roundtrips_each_bit_depth() {
    let cases = [
        (
            8,
            Case {
                channels: 1,
                frames: 4_096,
                sample_rate: 16_000,
            },
        ),
        (
            16,
            Case {
                channels: 2,
                frames: 8_192,
                sample_rate: 44_100,
            },
        ),
        (
            24,
            Case {
                channels: 1,
                frames: 8_192,
                sample_rate: 22_050,
            },
        ),
        (
            32,
            Case {
                channels: 2,
                frames: 6_321,
                sample_rate: 48_000,
            },
        ),
    ];

    for (bits_per_sample, case) in cases {
        assert_pcm_wav_roundtrip(case, bits_per_sample)
            .unwrap_or_else(|error| panic!("{bits_per_sample}-bit {case:?}: {error}"));
    }
}

#[test]
fn wav_encodes_to_the_upload_format_when_asked() -> TestResult {
    let tempdir = tempdir()?;
    let wav_path = tempdir.path().join("input.wav");
    let m4a_path = tempdir
        .path()
        .join(format!("upload.{}", audio_encode::UPLOAD_EXTENSION));
    let decoded_wav_path = tempdir.path().join("decoded.wav");
    let case = Case {
        channels: 2,
        frames: 16_000 * 3,
        sample_rate: 16_000,
    };

    let original = write_fixture_wav(&wav_path, case)?;
    encode_wav(&wav_path, &m4a_path)?;
    let bytes = std::fs::read(&m4a_path)?;
    assert_upload_container(&bytes);
    decode_to_wav(&m4a_path, &decoded_wav_path)?;
    let (spec, samples) = read_wav(&decoded_wav_path)?;
    assert_decoded_matches_case(case, original.len(), &spec, &samples);
    // No temporary file is left next to the output.
    assert_eq!(std::fs::read_dir(tempdir.path())?.count(), 3);
    Ok(())
}

#[test]
fn a_recording_stream_transcodes_to_a_stereo_upload_file() -> TestResult {
    let tempdir = tempdir()?;
    let wav_path = tempdir.path().join("input.wav");
    let aac_path = tempdir
        .path()
        .join(format!("audio.{}", audio_encode::RECORDING_EXTENSION));
    let m4a_path = tempdir
        .path()
        .join(format!("upload.{}", audio_encode::UPLOAD_EXTENSION));
    let decoded_wav_path = tempdir.path().join("decoded.wav");
    let case = Case {
        channels: 2,
        frames: 16_000 * 2,
        sample_rate: 16_000,
    };

    let original = write_fixture_wav(&wav_path, case)?;
    encode_wav(&wav_path, &aac_path)?;
    audio_encode::transcode_for_upload(&aac_path, &m4a_path)?;
    assert_upload_container(&std::fs::read(&m4a_path)?);
    decode_to_wav(&m4a_path, &decoded_wav_path)?;
    let (spec, samples) = read_wav(&decoded_wav_path)?;
    assert_eq!(spec.channels, 2, "stereo is kept for upload");
    assert_eq!(spec.sample_rate, case.sample_rate);
    assert_samples_valid(&samples);
    // Each AAC encode adds encoder delay and end padding (about 2112 + up to
    // 1024 frames); two encodes in a row may drift by twice that.
    let drift = original.len().abs_diff(samples.len()) / 2;
    assert!(drift <= 2 * 4096, "drift {drift} frames");
    Ok(())
}
