import { Trans } from "@lingui/react/macro";
import { useEffect, useState } from "react";

import { type AiUsageDay, loadAiUsage } from "~/ai/usage";

/** Read-only line with today's local AI usage counters. */
export function AiUsageLine() {
  const [usage, setUsage] = useState<AiUsageDay | null>(null);

  useEffect(() => {
    let active = true;
    void loadAiUsage()
      .then((day) => {
        if (active) setUsage(day);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  if (!usage) return null;

  const questions = usage.askQueries;
  const calls = usage.calls;
  const redirects = usage.askRedirects;

  return (
    <p data-ai-usage-line className="text-muted-foreground text-xs">
      <Trans>
        Today: {questions} Ask questions, {calls} AI calls, {redirects}{" "}
        redirects.
      </Trans>
    </p>
  );
}
