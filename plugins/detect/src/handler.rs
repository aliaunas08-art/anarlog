use tauri::{AppHandle, Manager, Runtime};
use tokio_util::sync::CancellationToken;

use crate::{
    DetectEvent, ProcessorState,
    env::{Env, TauriEnv},
    mic_usage_tracker,
};

pub fn setup<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn std::error::Error>> {
    let env = TauriEnv {
        app_handle: app.app_handle().clone(),
    };
    let processor = app.state::<ProcessorState>().inner().clone();

    let callback = anlg_detect::new_callback(move |event| {
        let env = env.clone();
        let processor = processor.clone();
        tauri::async_runtime::spawn(async move {
            handle_detect_event(&env, &processor, event);
        });
    });

    let detector_state = app.state::<crate::DetectorState>();
    let mut detector = detector_state.lock().unwrap_or_else(|e| e.into_inner());
    detector.start(callback);
    drop(detector);

    Ok(())
}

type SleepListener = Box<dyn Fn(bool) + Send + Sync>;

static SLEEP_LISTENERS: std::sync::Mutex<Vec<SleepListener>> = std::sync::Mutex::new(Vec::new());

/// Runs `listener` in Rust whenever the Mac is about to sleep (`true`) or
/// has woken (`false`). macOS only; never called elsewhere. Used by the app
/// lock to relock on sleep (ASPC A4.4).
pub fn on_sleep_state_changed(listener: impl Fn(bool) + Send + Sync + 'static) {
    SLEEP_LISTENERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Box::new(listener));
}

#[cfg_attr(not(all(target_os = "macos", feature = "sleep")), allow(dead_code))]
fn notify_sleep_listeners(asleep: bool) {
    for listener in SLEEP_LISTENERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
    {
        listener(asleep);
    }
}

pub fn handle_detect_event<E: Env>(
    env: &E,
    state: &ProcessorState,
    event: anlg_detect::DetectEvent,
) {
    match event {
        anlg_detect::DetectEvent::MicStarted(apps) => {
            if !env.is_detect_enabled() {
                return;
            }
            handle_mic_started(env, state, apps);
        }
        anlg_detect::DetectEvent::MicStopped(apps) => {
            handle_mic_stopped(env, state, apps);
        }
        #[cfg(all(target_os = "macos", feature = "zoom"))]
        anlg_detect::DetectEvent::ZoomMuteStateChanged { value } => {
            env.emit(DetectEvent::MicMuteStateChanged { value });
        }
        #[cfg(all(target_os = "macos", feature = "sleep"))]
        anlg_detect::DetectEvent::SleepStateChanged { value } => {
            notify_sleep_listeners(value);
            env.emit(DetectEvent::SleepStateChanged { value });
        }
    }
}

fn handle_mic_started<E: Env>(
    env: &E,
    state: &ProcessorState,
    apps: Vec<anlg_detect::InstalledApp>,
) {
    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());

    let to_track: Vec<_> = apps
        .iter()
        .filter(|app| {
            guard.policy.should_track_app(&app.id)
                && !guard.mic_usage_tracker.is_tracking(&app.id)
                && !guard.mic_usage_tracker.is_in_cooldown(&app.id)
        })
        .cloned()
        .collect();

    let threshold_secs = guard.mic_active_threshold_secs;

    for app in &to_track {
        let token = CancellationToken::new();
        let generation = guard
            .mic_usage_tracker
            .start_tracking(app.id.clone(), token.clone());
        mic_usage_tracker::spawn_timer(
            env.clone(),
            state.clone(),
            app.clone(),
            generation,
            token,
            threshold_secs,
        );
    }
}

fn handle_mic_stopped<E: Env>(
    env: &E,
    state: &ProcessorState,
    apps: Vec<anlg_detect::InstalledApp>,
) {
    {
        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());

        for app in &apps {
            guard.mic_usage_tracker.cancel_app(&app.id);
        }
    }

    env.emit(DetectEvent::MicStopped { apps });
}
