mod common;

use audio_encode::{decode_to_wav, encode_wav};
use tempfile::tempdir;

use crate::common::{
    Case, TestResult, write_fixture_wav, write_malformed_stereo_wav_with_odd_samples,
};

#[test]
fn rejects_more_than_two_channels() -> TestResult {
    let tempdir = tempdir()?;
    let wav_path = tempdir.path().join("input_3ch.wav");
    let aac_path = tempdir
        .path()
        .join(format!("encoded.{}", audio_encode::RECORDING_EXTENSION));

    write_fixture_wav(
        &wav_path,
        Case {
            channels: 3,
            frames: 128,
            sample_rate: 48_000,
        },
    )?;

    let err = encode_wav(&wav_path, &aac_path).expect_err("3-channel input should be rejected");
    let message = err.to_string();
    assert!(
        message.contains("unsupported channel count"),
        "unexpected error message: {message}"
    );

    Ok(())
}

#[test]
fn encode_rejects_malformed_stereo_data() -> TestResult {
    let tempdir = tempdir()?;
    let wav_path = tempdir.path().join("odd_stereo.wav");
    let aac_path = tempdir
        .path()
        .join(format!("encoded.{}", audio_encode::RECORDING_EXTENSION));

    write_malformed_stereo_wav_with_odd_samples(&wav_path)?;
    let err = encode_wav(&wav_path, &aac_path)
        .expect_err("malformed stereo wav should be rejected before encoding");
    let message = err.to_string();
    assert!(
        message.contains("invalid data chunk length"),
        "unexpected malformed wav error: {message}"
    );

    Ok(())
}

#[test]
fn decode_rejects_invalid_aac() -> TestResult {
    let tempdir = tempdir()?;
    let invalid_aac_path = tempdir
        .path()
        .join(format!("invalid.{}", audio_encode::RECORDING_EXTENSION));
    let wav_path = tempdir.path().join("decoded.wav");
    std::fs::write(&invalid_aac_path, b"not an aac stream")?;

    let result = decode_to_wav(&invalid_aac_path, &wav_path);
    assert!(result.is_err(), "invalid aac should return an error");

    Ok(())
}
