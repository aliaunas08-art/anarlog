// Neutral placeholder logo mark: a rounded-square note with ruled lines.
// public/assets/app-mark.svg and the boot splash in index.html mirror this
// glyph for places that cannot render React.
export const APP_MARK_PATH =
  "M8 3h8a5 5 0 0 1 5 5v8a5 5 0 0 1-5 5H8a5 5 0 0 1-5-5V8a5 5 0 0 1 5-5Zm-.5 5.25v1.5h9v-1.5h-9Zm0 3v1.5h9v-1.5h-9Zm0 3v1.5h6v-1.5h-6Z";

export const APP_MARK_ICON_SRC = "/assets/app-mark.svg";

export function AppMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="3 3 18 18"
      fill="currentColor"
      className={className}
      aria-hidden="true"
    >
      <path fillRule="evenodd" d={APP_MARK_PATH} />
    </svg>
  );
}
