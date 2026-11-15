import { platform } from "@tauri-apps/plugin-os";

import { cn } from "@anlg/utils";

import { SyncProvider } from "~/calendar/components/context";
import { usesWindowsStyleTitleBar } from "~/shared/hooks/useWindowControlsGutter";

export type MainSurfaceChrome = "default" | "top" | "top-borderless" | "left";

export function MainShellScaffold({
  children,
  edgeToEdge = false,
  mainSurfaceChrome,
}: {
  children: React.ReactNode;
  edgeToEdge?: boolean;
  mainSurfaceChrome?: MainSurfaceChrome;
}) {
  const runtimePlatform = platform();
  const isMacos = runtimePlatform === "macos";
  const hasCustomTitleBar = usesWindowsStyleTitleBar();
  const resolvedMainSurfaceChrome =
    mainSurfaceChrome ?? (edgeToEdge ? "top" : "default");
  const hasTopMainSurfaceChrome =
    resolvedMainSurfaceChrome === "top" ||
    resolvedMainSurfaceChrome === "top-borderless";

  return (
    <SyncProvider>
      <div
        className={cn([
          "bg-background flex h-full gap-1 overflow-hidden",
          !hasTopMainSurfaceChrome && "pl-1",
          hasTopMainSurfaceChrome && [
            isMacos && "[&_[data-chat-floating-anchor]]:rounded-t-xl",
            "[&_[data-chat-floating-anchor]]:rounded-b-none",
            "[&_[data-chat-floating-anchor]]:border-x-0",
            resolvedMainSurfaceChrome === "top" || hasCustomTitleBar
              ? "[&_[data-chat-floating-anchor]]:border-t"
              : "[&_[data-chat-floating-anchor]]:!border-t-0",
            "[&_[data-chat-floating-anchor]]:border-b-0",
          ],
          // C · Final: next to the floating sidebar panel the note sits
          // flat on the window, with no card edge.
          resolvedMainSurfaceChrome === "left" && [
            "[&_[data-chat-floating-anchor]]:rounded-none",
            hasCustomTitleBar
              ? [
                  "[&_[data-chat-floating-anchor]]:border-x-0",
                  "[&_[data-chat-floating-anchor]]:border-t",
                  "[&_[data-chat-floating-anchor]]:border-b-0",
                ]
              : "[&_[data-chat-floating-anchor]]:border-0",
          ],
          hasTopMainSurfaceChrome &&
            !isMacos &&
            "[&_[data-chat-floating-anchor]]:rounded-t-xl",
        ])}
        data-testid="main-app-shell"
      >
        {children}
      </div>
    </SyncProvider>
  );
}
