import { lazy, Suspense } from "react";

// The devtools bar is left out of production bundles. Vite replaces
// import.meta.env.DEV with false in `vite build`, so this dynamic import and
// everything behind it (react-scan, and through it react-doctor, whose
// modified MIT licence restricts hosted resale) never reach a release build.
const DevtoolsStatusBar = import.meta.env.DEV
  ? lazy(() =>
      import("~/devtools-bar").then((module) => ({
        default: module.DevtoolsStatusBar,
      })),
    )
  : null;

export function DevtoolsSlot() {
  if (!DevtoolsStatusBar) return null;
  return (
    <Suspense fallback={null}>
      <DevtoolsStatusBar />
    </Suspense>
  );
}
