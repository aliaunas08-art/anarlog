import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";

import { Button } from "@anlg/ui/components/ui/button";

import { DestructiveConfirmationDialog } from "~/shared/ui/destructive-confirmation-dialog";
import {
  cancelVoiceEnrollment,
  deleteVoiceEnrollment,
  describeSpeakerCommandError,
  finishVoiceEnrollment,
  getVoiceEnrollmentStatus,
  startVoiceEnrollment,
  type VoiceEnrollmentStatus,
} from "~/speakers/api";

export const VOICE_ENROLLMENT_QUERY_KEY = ["voice-enrollment-status"] as const;
export const VOICE_ENROLLMENT_TARGET_SECONDS = 30;

type Phase = "idle" | "prompt" | "recording" | "finishing";

export function useVoiceEnrollmentStatus() {
  return useQuery({
    queryKey: VOICE_ENROLLMENT_QUERY_KEY,
    queryFn: getVoiceEnrollmentStatus,
    retry: false,
  });
}

export function VoiceProfileSection({
  onEditName,
}: {
  onEditName?: () => void;
}) {
  const { t, i18n } = useLingui();
  const queryClient = useQueryClient();
  const status = useVoiceEnrollmentStatus();
  const [phase, setPhase] = useState<Phase>("idle");
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  const setStatus = useCallback(
    (next: VoiceEnrollmentStatus) =>
      queryClient.setQueryData(VOICE_ENROLLMENT_QUERY_KEY, next),
    [queryClient],
  );

  const finish = useCallback(async () => {
    if (phaseRef.current !== "recording") return;
    setPhase("finishing");
    try {
      setStatus(await finishVoiceEnrollment());
      setPhase("idle");
    } catch (finishError) {
      setError(describeSpeakerCommandError(finishError));
      setPhase("prompt");
    }
  }, [setStatus]);

  useEffect(() => {
    if (phase !== "recording") return;
    const startedAt = Date.now();
    setElapsed(0);
    const timer = setInterval(() => {
      const seconds = (Date.now() - startedAt) / 1000;
      setElapsed(Math.min(seconds, VOICE_ENROLLMENT_TARGET_SECONDS));
      if (seconds >= VOICE_ENROLLMENT_TARGET_SECONDS) void finish();
    }, 250);
    return () => clearInterval(timer);
  }, [finish, phase]);

  // Leaving the page mid-recording must not leave the microphone open.
  useEffect(
    () => () => {
      if (phaseRef.current === "recording") void cancelVoiceEnrollment();
    },
    [],
  );

  const record = async () => {
    setError(null);
    try {
      await startVoiceEnrollment();
      setPhase("recording");
    } catch (startError) {
      setError(describeSpeakerCommandError(startError));
    }
  };

  const cancel = async () => {
    const wasRecording = phaseRef.current === "recording";
    setPhase("idle");
    setError(null);
    if (wasRecording) {
      await cancelVoiceEnrollment().catch((cancelError) =>
        console.error("[voice-profile] cancel failed", cancelError),
      );
    }
  };

  const remove = async () => {
    setDeleting(true);
    try {
      await deleteVoiceEnrollment();
      setStatus({
        enrolled: false,
        createdAtMs: null,
        updatedAtMs: null,
        speechSeconds: null,
      });
      setConfirmDelete(false);
    } catch (deleteError) {
      setError(describeSpeakerCommandError(deleteError));
    } finally {
      setDeleting(false);
    }
  };

  const enrolled = status.data?.enrolled === true;
  const setUpAtMs = status.data?.updatedAtMs ?? status.data?.createdAtMs;
  const setUpDate = setUpAtMs
    ? new Date(setUpAtMs).toLocaleDateString(i18n.locale, {
        dateStyle: "medium",
      })
    : null;

  return (
    <section
      id="voice-profile"
      aria-labelledby="voice-profile-title"
      className="flex flex-col gap-4"
    >
      <div className="flex flex-col gap-2">
        <h3 id="voice-profile-title" className="text-sm font-medium">
          <Trans>Voice profile</Trans>
        </h3>
        <p className="text-sm text-[var(--cf-secondary)]">
          <Trans>
            Your voice profile is biometric data. It's optional, it stays only
            on this Mac in your Keychain, it's never uploaded, synced, or
            included in exports, and you can delete or re-record it any time.
            It's used only to label your own voice in in-person meetings.
          </Trans>
        </p>
      </div>

      <div className="divide-y divide-[var(--cf-separator)] overflow-hidden rounded-xl bg-[var(--cf-fill)]">
        {onEditName ? (
          <Row
            title={<Trans>Your name</Trans>}
            detail={<Trans>Used to label your lines in transcripts.</Trans>}
          >
            <Button
              variant="ghost"
              size="sm"
              className="rounded-pill text-[var(--cf-accent)]"
              onClick={onEditName}
            >
              <Trans>Edit name</Trans>
            </Button>
          </Row>
        ) : null}

        {phase === "idle" ? (
          <Row
            title={<Trans>Voice profile</Trans>}
            detail={
              status.isLoading ? (
                <Trans>Checking…</Trans>
              ) : status.isError ? (
                <Trans>Not available in this build.</Trans>
              ) : enrolled ? (
                setUpDate ? (
                  <Trans>Set up on {setUpDate}</Trans>
                ) : (
                  <Trans>Set up</Trans>
                )
              ) : (
                <Trans>Not set up</Trans>
              )
            }
          >
            {enrolled ? (
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  className="rounded-pill"
                  onClick={() => setPhase("prompt")}
                >
                  <Trans>Re-record</Trans>
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="rounded-pill text-destructive"
                  onClick={() => setConfirmDelete(true)}
                >
                  <Trans>Delete</Trans>
                </Button>
              </div>
            ) : (
              <Button
                size="sm"
                className="rounded-pill"
                disabled={status.isLoading || status.isError}
                onClick={() => setPhase("prompt")}
              >
                <Trans>Set up</Trans>
              </Button>
            )}
          </Row>
        ) : (
          <div className="flex flex-col gap-3 px-4 py-3">
            <p className="text-xs text-[var(--cf-secondary)]">
              {phase === "prompt" ? (
                <Trans>
                  When you're ready, press Record and read this aloud at your
                  normal pace. It takes about 30 seconds.
                </Trans>
              ) : (
                <Trans>Keep reading at your normal pace.</Trans>
              )}
            </p>
            <blockquote className="text-[15px] leading-[1.5] text-[var(--cf-label)]">
              <Trans>
                The morning train was quieter than usual, so I had time to look
                out of the window. Fields gave way to small towns, then to the
                edge of the city. I thought about the week ahead: a few
                meetings, a long list of small tasks, and one project I am
                looking forward to finishing. By the time we arrived, the clouds
                had cleared and the station was full of people heading in every
                direction.
              </Trans>
            </blockquote>
            {phase !== "prompt" ? (
              <div className="flex items-center gap-3">
                <div
                  role="progressbar"
                  aria-label={t`Recording progress`}
                  aria-valuemin={0}
                  aria-valuemax={VOICE_ENROLLMENT_TARGET_SECONDS}
                  aria-valuenow={Math.round(elapsed)}
                  className="rounded-pill h-1 flex-1 overflow-hidden bg-[var(--cf-separator)]"
                >
                  <div
                    className="rounded-pill h-full bg-[var(--cf-accent)] transition-[width] duration-200"
                    style={{
                      width: `${(elapsed / VOICE_ENROLLMENT_TARGET_SECONDS) * 100}%`,
                    }}
                  />
                </div>
                <span className="w-10 text-right text-xs text-[var(--cf-tertiary)] tabular-nums">
                  {formatSeconds(elapsed)}
                </span>
              </div>
            ) : null}
            {error ? (
              <p role="status" className="text-xs text-[var(--cf-secondary)]">
                {error}
              </p>
            ) : null}
            <div className="flex items-center justify-end gap-2">
              <Button
                variant="ghost"
                size="sm"
                className="rounded-pill"
                disabled={phase === "finishing"}
                onClick={() => void cancel()}
              >
                <Trans>Cancel</Trans>
              </Button>
              {phase === "prompt" ? (
                <Button
                  size="sm"
                  className="rounded-pill"
                  onClick={() => void record()}
                >
                  <Trans>Record</Trans>
                </Button>
              ) : (
                <Button
                  size="sm"
                  className="rounded-pill"
                  disabled={phase === "finishing"}
                  onClick={() => void finish()}
                >
                  {phase === "finishing" ? (
                    <Trans>Saving…</Trans>
                  ) : (
                    <Trans>Done</Trans>
                  )}
                </Button>
              )}
            </div>
          </div>
        )}
      </div>

      {phase === "idle" && error ? (
        <p role="status" className="text-xs text-[var(--cf-secondary)]">
          {error}
        </p>
      ) : null}

      <DestructiveConfirmationDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={t`Delete your voice profile?`}
        description={t`Your voice profile will be removed from this Mac. Your lines in in-person meetings will no longer be labelled automatically.`}
        confirmLabel={t`Delete`}
        pendingLabel={t`Deleting…`}
        isPending={deleting}
        onConfirm={() => void remove()}
      />
    </section>
  );
}

function Row({
  title,
  detail,
  children,
}: {
  title: ReactNode;
  detail?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="flex min-h-11 items-center gap-4 px-4 py-2">
      <div className="min-w-0 flex-1">
        <div className="text-sm text-[var(--cf-label)]">{title}</div>
        {detail ? (
          <div className="text-xs text-[var(--cf-secondary)]">{detail}</div>
        ) : null}
      </div>
      {children ? <div className="shrink-0">{children}</div> : null}
    </div>
  );
}

function formatSeconds(seconds: number) {
  const whole = Math.floor(seconds);
  return `0:${String(whole).padStart(2, "0")}`;
}
