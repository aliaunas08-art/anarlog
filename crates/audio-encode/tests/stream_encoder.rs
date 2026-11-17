mod common;

use audio_encode::{MonoStreamEncoder, StereoStreamEncoder};

use crate::common::{
    Case, TestResult, assert_decoded_matches_case, decode_aac_bytes, fixture_channel_f32,
    fixture_channel_i16,
};

#[test]
fn mono_stream_encoder_f32_roundtrips_across_multiple_chunks() -> TestResult {
    let case = Case {
        channels: 1,
        frames: 6_500,
        sample_rate: 16_000,
    };
    let samples = fixture_channel_f32(case.frames, 0);
    let mut encoder = MonoStreamEncoder::new(case.sample_rate)?;
    let mut output = Vec::new();

    encoder.encode_f32(&samples[..1_024], &mut output)?;
    encoder.encode_f32(&samples[1_024..5_000], &mut output)?;
    encoder.encode_f32(&samples[5_000..], &mut output)?;
    encoder.flush(&mut output)?;

    assert!(!output.is_empty(), "streaming encoder produced no aac data");

    let (decoded_spec, decoded_samples) = decode_aac_bytes(&output)?;
    assert_decoded_matches_case(case, samples.len(), &decoded_spec, &decoded_samples);
    Ok(())
}

#[test]
fn stereo_stream_encoder_f32_roundtrips_across_multiple_chunks() -> TestResult {
    let case = Case {
        channels: 2,
        frames: 7_000,
        sample_rate: 48_000,
    };
    let left = fixture_channel_f32(case.frames, 0);
    let right = fixture_channel_f32(case.frames, 1);
    let mut encoder = StereoStreamEncoder::new(case.sample_rate)?;
    let mut output = Vec::new();

    encoder.encode_f32(&left[..2_048], &right[..2_048], &mut output)?;
    encoder.encode_f32(&left[2_048..5_120], &right[2_048..5_120], &mut output)?;
    encoder.encode_f32(&left[5_120..], &right[5_120..], &mut output)?;
    encoder.flush(&mut output)?;

    assert!(!output.is_empty(), "streaming encoder produced no aac data");

    let (decoded_spec, decoded_samples) = decode_aac_bytes(&output)?;
    assert_decoded_matches_case(case, case.frames * 2, &decoded_spec, &decoded_samples);
    Ok(())
}

#[test]
fn mono_stream_encoder_i16_roundtrips() -> TestResult {
    let case = Case {
        channels: 1,
        frames: 5_000,
        sample_rate: 22_050,
    };
    let samples = fixture_channel_i16(case.frames, 0);
    let mut encoder = MonoStreamEncoder::new(case.sample_rate)?;
    let mut output = Vec::new();

    encoder.encode_i16(&samples[..2_048], &mut output)?;
    encoder.encode_i16(&samples[2_048..], &mut output)?;
    encoder.flush(&mut output)?;

    assert!(!output.is_empty(), "streaming encoder produced no aac data");

    let (decoded_spec, decoded_samples) = decode_aac_bytes(&output)?;
    assert_decoded_matches_case(case, samples.len(), &decoded_spec, &decoded_samples);
    Ok(())
}

#[test]
fn stereo_stream_encoder_i16_roundtrips() -> TestResult {
    let case = Case {
        channels: 2,
        frames: 4_800,
        sample_rate: 44_100,
    };
    let left = fixture_channel_i16(case.frames, 0);
    let right = fixture_channel_i16(case.frames, 1);
    let mut encoder = StereoStreamEncoder::new(case.sample_rate)?;
    let mut output = Vec::new();

    encoder.encode_i16(&left[..1_024], &right[..1_024], &mut output)?;
    encoder.encode_i16(&left[1_024..], &right[1_024..], &mut output)?;
    encoder.flush(&mut output)?;

    assert!(!output.is_empty(), "streaming encoder produced no aac data");

    let (decoded_spec, decoded_samples) = decode_aac_bytes(&output)?;
    assert_decoded_matches_case(case, case.frames * 2, &decoded_spec, &decoded_samples);
    Ok(())
}

#[test]
fn mono_stream_encoder_empty_flush_produces_decodable_silence() -> TestResult {
    let case = Case {
        channels: 1,
        frames: 0,
        sample_rate: 16_000,
    };
    let mut encoder = MonoStreamEncoder::new(case.sample_rate)?;
    let mut output = Vec::new();

    encoder.encode_f32(&[], &mut output)?;
    encoder.encode_i16(&[], &mut output)?;
    encoder.flush(&mut output)?;

    assert!(
        !output.is_empty(),
        "flush-only stream should still emit aac data"
    );

    let (decoded_spec, decoded_samples) = decode_aac_bytes(&output)?;
    assert_decoded_matches_case(case, 0, &decoded_spec, &decoded_samples);
    Ok(())
}

#[test]
fn stereo_stream_encoder_zero_pads_short_right_channel() -> TestResult {
    let sample_rate = 48_000;
    let left = fixture_channel_f32(513, 0);
    let right = fixture_channel_f32(512, 1);
    let mut padded_right = right.clone();
    padded_right.push(0.0);

    let mut short_encoder = StereoStreamEncoder::new(sample_rate)?;
    let mut short_output = Vec::new();
    short_encoder.encode_f32(&left, &right, &mut short_output)?;
    short_encoder.flush(&mut short_output)?;

    let mut padded_encoder = StereoStreamEncoder::new(sample_rate)?;
    let mut padded_output = Vec::new();
    padded_encoder.encode_f32(&left, &padded_right, &mut padded_output)?;
    padded_encoder.flush(&mut padded_output)?;

    assert_eq!(short_output, padded_output);
    Ok(())
}

#[test]
fn appending_to_an_existing_recording_keeps_one_decodable_stream() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(audio_encode::RECORDING_FILE);
    let tone: Vec<f32> = (0..16_000).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();

    let mut first = StereoStreamEncoder::new(16_000)?;
    let mut bytes = Vec::new();
    first.encode_f32(&tone, &tone, &mut bytes)?;
    first.flush(&mut bytes)?;
    std::fs::write(&path, &bytes)?;

    // A meeting records again: the new stream is appended to the file.
    let mut next = StereoStreamEncoder::appending_to(16_000, &path)?;
    let mut appended = Vec::new();
    next.encode_f32(&tone, &tone, &mut appended)?;
    next.flush(&mut appended)?;
    let mut file = std::fs::OpenOptions::new().append(true).open(&path)?;
    std::io::Write::write_all(&mut file, &appended)?;
    drop(file);

    let (spec, samples) = common::decode_audio_file(&path)?;
    assert_eq!(spec.channels, 2);
    let frames = samples.len() / 2;
    // Two seconds of audio, plus each stream's codec overhead.
    assert!(
        (32_000..32_000 + 2 * 4_096).contains(&frames),
        "{frames} frames"
    );
    Ok(())
}
