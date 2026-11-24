import { useLingui } from "@lingui/react/macro";
import { useCallback, useRef, useState } from "react";

import { commands as deeplinkCommands } from "@anlg/plugin-deeplink2";
import { commands as openerCommands } from "@anlg/plugin-opener2";
import { Headset, VideoCamera } from "@anlg/ui/components/icons";
import { parseEventInstant, safeParseDate } from "@anlg/utils";

import { useHasTranscript } from "../shared";

import { useAudioPlayer } from "~/audio-player";
import { useNow } from "~/calendar/hooks";
import {
  buildWelcomeNoteDemoUrl,
  WELCOME_NOTE_TRACKING_ID,
} from "~/onboarding/welcome-note.constants";
import { useEventCountdown } from "~/session/hooks/useEventCountdown";
import { useMeetingMicInUse } from "~/session/hooks/useMeetingMicInUse";
import {
  getRemoteMeeting,
  type RemoteMeeting,
} from "~/session/hooks/useRemoteMeeting";
import { useSessionEvent } from "~/session/hooks/useSessionEvent";
import { getScheme } from "~/shared/utils";
import { useListener } from "~/stt/contexts";
import { useStartListening } from "~/stt/useStartListening";
import {
  isMainWebviewWindow,
  requestMainListenerControl,
} from "~/stt/window-control";

export type SessionMeetingAction = {
  kind: "record" | "join" | "stop";
  label: string;
  title: string;
  /** Meeting app logo for join; record draws its own dot. */
  icon?: React.ReactNode;
  onClick: () => void;
  disabled: boolean;
  showInPerson: boolean;
  startInPerson: () => void;
  countdownLabel: string | null;
  showWelcomeDemoPrompt: boolean;
};

function meetingHasStarted(startedAt: string | undefined, now: Date) {
  const start = parseEventInstant(startedAt);
  return start != null && now.getTime() >= start.getTime();
}

/**
 * Primary recording action for a session (Record / Join & record / Stop),
 * shared by the session bottom bar. Returns null while the session is
 * finalizing or transcribing after the meeting, when there is nothing to do.
 */
export function useSessionMeetingAction(
  sessionId: string,
): SessionMeetingAction | null {
  const { t } = useLingui();
  const sessionMode = useListener((state) => state.getSessionMode(sessionId));
  const stop = useListener((state) => state.stop);
  const event = useSessionEvent(sessionId);
  const hasTranscript = useHasTranscript(sessionId);
  const { audioExists } = useAudioPlayer();
  const now = useNow();
  const startListening = useStartListening(sessionId);
  const countdown = useEventCountdown(sessionId);

  const endedAt = event?.ended_at ? safeParseDate(event.ended_at) : null;
  const eventEnded = !!endedAt && endedAt.getTime() <= now.getTime();
  const remote = getRemoteMeeting(event?.meeting_link);
  const meetingLink = event?.meeting_link || null;
  const isWelcomeDemo = event?.tracking_id === WELCOME_NOTE_TRACKING_ID;
  const canJoin = Boolean(
    !eventEnded &&
    !hasTranscript &&
    !audioExists &&
    meetingLink &&
    (remote !== null || isWelcomeDemo),
  );
  const meetingStarted = meetingHasStarted(event?.started_at, now);
  const meetingMicInUse = useMeetingMicInUse(
    canJoin && !isWelcomeDemo && sessionMode === "inactive" && meetingStarted,
  );

  const joiningMeetingRef = useRef(false);
  const [joiningMeeting, setJoiningMeeting] = useState(false);

  const start = useCallback(async () => {
    if (!isMainWebviewWindow()) {
      await requestMainListenerControl("start", sessionId);
      return;
    }

    await startListening();
  }, [sessionId, startListening]);
  const startRecording = useCallback(() => {
    void start();
  }, [start]);
  const startInPerson = useCallback(() => {
    void startListening({ micOnly: true });
  }, [startListening]);
  const openMeeting = useCallback(async () => {
    if (!meetingLink) {
      return;
    }

    let url = meetingLink;
    if (isWelcomeDemo) {
      url = buildWelcomeNoteDemoUrl(meetingLink);
      try {
        const scheme = await getScheme();
        const result = await deeplinkCommands.startCallbackServer(scheme, null);
        if (result.status === "ok") {
          url = buildWelcomeNoteDemoUrl(meetingLink, result.data);
        }
      } catch (error) {
        console.error(
          "[onboarding] failed to prepare demo completion callback",
          error,
        );
      }
    }

    void openerCommands.openUrl(url, null);
  }, [isWelcomeDemo, meetingLink]);
  const joinMeeting = useCallback(async () => {
    if (joiningMeetingRef.current) {
      return;
    }

    joiningMeetingRef.current = true;
    setJoiningMeeting(true);
    try {
      await Promise.all([openMeeting(), start()]);
    } finally {
      joiningMeetingRef.current = false;
      setJoiningMeeting(false);
    }
  }, [openMeeting, start]);
  const stopListening = useCallback(() => {
    if (!isMainWebviewWindow()) {
      void requestMainListenerControl("stop", sessionId);
      return;
    }

    stop();
  }, [sessionId, stop]);

  if (sessionMode === "finalizing" || sessionMode === "running_batch") {
    return null;
  }

  const base = (() => {
    if (sessionMode === "active") {
      return {
        kind: "stop" as const,
        label: t`Stop`,
        title: t`Stop listening`,
        onClick: stopListening,
      };
    }

    if (canJoin && (isWelcomeDemo || !meetingStarted || !meetingMicInUse)) {
      return {
        kind: "join" as const,
        label: t`Join & record`,
        title: t`Join meeting and record`,
        icon: isWelcomeDemo ? (
          <img
            src="/assets/app-mark.svg"
            alt=""
            className="size-3.5 shrink-0"
          />
        ) : remote ? (
          getMeetingDisplay(remote.type).icon
        ) : undefined,
        onClick: () => {
          void joinMeeting();
        },
      };
    }

    return {
      kind: "record" as const,
      label: t`Record`,
      title: hasTranscript || audioExists ? t`Resume recording` : t`Record`,
      onClick: startRecording,
    };
  })();

  const inactive = sessionMode === "inactive";

  return {
    ...base,
    disabled: joiningMeeting,
    showInPerson: inactive && base.kind === "record" && isMainWebviewWindow(),
    startInPerson,
    countdownLabel:
      countdown.label && sessionMode !== "active" ? countdown.label : null,
    showWelcomeDemoPrompt:
      isWelcomeDemo && inactive && !hasTranscript && !audioExists,
  };
}

export function getMeetingDisplay(type: RemoteMeeting["type"]) {
  switch (type) {
    case "zoom":
      return {
        name: "Zoom",
        icon: (
          <img
            src="/assets/zoom-icon.svg"
            alt=""
            className="size-3.5 shrink-0"
          />
        ),
      };
    case "google-meet":
      return {
        name: "Google Meet",
        icon: (
          <img
            src="/assets/google-meet.svg"
            alt=""
            className="size-3.5 shrink-0"
          />
        ),
      };
    case "webex":
      return {
        name: "Webex",
        icon: (
          <img src="/assets/webex.png" alt="" className="size-3.5 shrink-0" />
        ),
      };
    case "teams":
      return {
        name: "Teams",
        icon: (
          <img src="/assets/teams.png" alt="" className="size-3.5 shrink-0" />
        ),
      };
    case "cal-com":
      return {
        name: "Cal.com",
        icon: <VideoCamera className="size-3.5 shrink-0" />,
      };
    case "x-call":
      return {
        name: "X",
        icon: (
          <img
            src="/assets/X logo.svg"
            alt=""
            className="size-3.5 shrink-0 dark:invert"
          />
        ),
      };
    default:
      return {
        name: "Meeting",
        icon: <Headset className="size-3.5 shrink-0" />,
      };
  }
}
