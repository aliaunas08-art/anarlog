# Handoff (2026-11-17)

Branch `prototype`. Nothing pushed. Local commits from this work:

- `0f11c079`: LAME removed. Recordings are AAC through macOS AudioToolbox; Windows and Linux use FLAC. Old `.mp3` recordings are still read.
- `1f1210e9`: fix for the app staying on the splash screen at cold start while the lock is on. The SQL authorizer refused Rust's own BEGIN inside the guarded read.

Details are in `docs/ASPC_AUDIT.md` ("LAME removal (D5, 2026-11-17)") and `docs/ASPC_BASE.md` D5.3.

## (a) LAME → AAC recording check: PASS

This was a real recording in the dev app, using on-device Whisper (BYO-model, `ggml-base.en-q8_0.bin`) for this test only.

| Check | Evidence |
|---|---|
| File is in the meeting folder | `sessions/7b28d7b3-2aa0-4250-abad-7058e0336981/audio.aac`, 1,025,817 bytes |
| New format | Starts with ADTS sync bytes `FF F1`. `afinfo` reports: `adts`, AAC, 2 ch, 16 kHz, about 80 kbit/s |
| Plays back | `afconvert` decodes it to 102.8 s of PCM (1606 packets). Peak 8030, RMS 240, so it is not silent |
| Whisper transcript | 2 `batch_transcription` rows, provider `whispercpp`, model `local-file`: 22 words and 210 words |

The `.m4a` upload file isn't used here: whisper.cpp reads the `.aac` directly. The `.m4a` path is covered by the `audio-encode` and `listener2-core` tests.

The engine setting change was made in Settings only, never in code, and the app's default engine is unchanged. It was set back to **Apple Speech** after the test (confirmed in the settings, 2026-11-17 23:11 UTC). A copy of the model is in `~/Downloads/ggml-base.en-q8_0.bin` (the picker couldn't see the app folder). You can delete it once the engine is back on Apple Speech.

## (b) Follow-up bug, NOT fixed here: Apple Speech crashes when recording starts

- Crash reports: `~/Library/Logs/DiagnosticReports/desktop-2026-11-17-165524.ips`, `-165802.ips`, `-171606.ips`.
- Error: `EXC_BAD_ACCESS (KERN_INVALID_ADDRESS)`.
- Crashed thread, from the top:
  - `swift_release` / `doDecrementSlow`
  - `outlined destroy of StatusPayload`
  - `SpeechAnalyzerBridge.startLiveJSON(locale:)`
  - `closure in _appleSpeechLiveStart(locale:)`
  - `waitForValue`
- Code area: `transcribe-speechanalyzer` (Swift bridge). It crashes every time live transcription starts with Apple Speech.
- Not yet checked whether it predates the LAME swap. Guess: it is unrelated, because the crash is in the Swift live-start path, not the audio encoder.

## Design issue to log with (b): a transcription crash destroys the recording

The Apple Speech crash left an **empty** meeting folder (`sessions/8d25ea5d…`). The whole process died at the start of live transcription, so no audio was kept. Audio capture should survive a transcription crash. Ways to do that include:

- running the speech engine in a separate process;
- starting capture and writing `audio.aac` before the engine starts, so the crash-recovery path has something to recover.

Noted only; not fixed in this task.

## Other open items

- `listener2-core` test `apple_speech_language_support_reflects_installed_framework` fails on this Mac. It depends on which speech languages are installed. Not yet checked whether it fails on the code before this change.
- Known differences after the LAME swap are listed in ASPC_AUDIT.md:
  - recordings start about 60 ms later;
  - older app versions don't sync `.aac` or `.flac`;
  - FLAC files are larger;
  - the FLAC path is not built or tested on real Windows or Linux here.
- The licence check still fails on `pos` (LGPL) and `khroma` (unknown licence). The allowlist was not changed, as instructed.
