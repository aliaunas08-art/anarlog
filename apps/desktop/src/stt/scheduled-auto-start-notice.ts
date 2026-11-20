import { t } from "@lingui/core/macro";

import { commands as notificationCommands } from "@anlg/plugin-notification";

import { APP_NAME } from "~/shared/app-name";

// ASPC C6.5: a recording started automatically from the calendar always
// shows a system notification, whatever the notification settings, so it
// never starts unnoticed while the app is hidden. The menu-bar indicator is
// forced on in Rust while recording (C6.1). The text names no meeting.
export async function notifyScheduledRecordingStarted(
  sessionId: string,
): Promise<void> {
  const result = await notificationCommands.showNotification({
    key: `scheduled-recording-started-${sessionId}`,
    title: t`Recording started automatically`,
    message: t`${APP_NAME} started recording a calendar meeting. Stop it from the menu bar or the floating bar.`,
    timeout: { secs: 15, nanos: 0 },
    source: null,
    start_time: null,
    participants: null,
    event_details: null,
    action_label: null,
    action: null,
    action_menu: null,
    action_variant: null,
    options: null,
    footer: null,
    icon: null,
  });
  if (result.status === "error") {
    console.error("[listener] failed to show the auto-start notification");
  }
}
