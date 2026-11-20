import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AudioSettingsView } from "./audio-settings";

function renderAudioSettings({
  microphoneDevice = {
    value: "",
    devices: ["External Microphone"],
    onChange: vi.fn(),
  },
  onOpenVoiceProfile = vi.fn(),
} = {}) {
  return render(
    <AudioSettingsView
      audioRetention={{ value: "forever", onChange: vi.fn() }}
      microphoneDevice={microphoneDevice}
      onOpenVoiceProfile={onOpenVoiceProfile}
    />,
  );
}

describe("AudioSettingsView", () => {
  afterEach(cleanup);

  it("shows when the selected microphone is unavailable and will fall back", () => {
    renderAudioSettings({
      microphoneDevice: {
        value: "Disconnected Microphone",
        devices: ["External Microphone"],
        onChange: vi.fn(),
      },
    });

    expect(
      screen.getByRole("combobox", { name: "Microphone" }).textContent,
    ).toContain(
      "Disconnected Microphone (Unavailable — using current default)",
    );
  });

  it("opens the voice profile instead of a remember-speakers toggle", () => {
    const onOpenVoiceProfile = vi.fn();
    renderAudioSettings({ onOpenVoiceProfile });

    expect(screen.queryByRole("switch", { name: "Remember speakers" })).toBe(
      null,
    );
    screen.getByRole("button", { name: "Voice profile" }).click();
    expect(onOpenVoiceProfile).toHaveBeenCalledTimes(1);
  });
});
