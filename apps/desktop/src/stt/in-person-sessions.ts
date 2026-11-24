// Sessions started with the in-person button. Resume/restart paths call
// startListening() without options; they must not silently fall back to
// meeting capture (system audio). In-memory only: cleared on app restart.
const inPersonSessionIds = new Set<string>();

/** Mark a session as in-person (mic only) before its recording auto-starts. */
export function markSessionInPerson(sessionId: string) {
  inPersonSessionIds.add(sessionId);
}

/** Whether this session was started in person during this app run. */
export function isSessionMarkedInPerson(sessionId: string) {
  return inPersonSessionIds.has(sessionId);
}
