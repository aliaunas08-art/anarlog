use std::sync::mpsc;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};

use crate::{Outcome, PROMPT_TIMEOUT};

pub fn authenticate(reason: &str) -> Outcome {
    // SAFETY: LAContext is thread-safe to create and query; the reply block
    // runs on a LocalAuthentication queue and only sends on a channel.
    let context = unsafe { LAContext::new() };
    if unsafe { context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) }.is_err() {
        return Outcome::Unavailable;
    }
    let (tx, rx) = mpsc::channel();
    let keep_alive = context.clone();
    let reply = RcBlock::new(move |success: Bool, error: *mut NSError| {
        let _keep_alive = &keep_alive;
        if !success.as_bool() {
            // LAError code only (for example -2 user cancel, -1004 not
            // interactive): says why nothing was unlocked.
            let code = unsafe { error.as_ref() }.map(|error| error.code());
            tracing::warn!(?code, "device_auth_not_confirmed");
        }
        let _ = tx.send(success.as_bool());
    });
    let localized_reason = NSString::from_str(reason);
    unsafe {
        context.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthentication,
            &localized_reason,
            &reply,
        );
    }
    match rx.recv_timeout(PROMPT_TIMEOUT) {
        Ok(true) => Outcome::Confirmed,
        _ => Outcome::NotConfirmed,
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
}

pub fn system_idle_seconds() -> Option<f64> {
    // kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType
    const COMBINED_SESSION_STATE: i32 = 0;
    const ANY_INPUT_EVENT: u32 = u32::MAX;
    // SAFETY: a pure query with constant arguments; it reads no pointers.
    let seconds =
        unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}
