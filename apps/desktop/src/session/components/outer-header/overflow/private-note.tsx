import { Trans, useLingui } from "@lingui/react/macro";

import { commands } from "@anlg/plugin-session";
import { Eye, EyeSlash } from "@anlg/ui/components/icons";
import { DropdownMenuItem } from "@anlg/ui/components/ui/dropdown-menu";
import { toast } from "@anlg/ui/components/ui/toast";

import { useSession } from "~/session/queries";

// ASPC C6.4: a Private meeting is left out of all-meetings Ask, bulk
// exports and sharing unless the user includes it for that one action.
// Separate from Lock Note, which hides the content behind device auth.
// Rust writes the flag so each change is audit-logged (D1.4).
export function PrivateNote({ sessionId }: { sessionId: string }) {
  const { t } = useLingui();
  const session = useSession(sessionId);
  const isPrivate = session?.private === true;

  return (
    <DropdownMenuItem
      onClick={(e) => {
        e.preventDefault();
        void commands
          .setSessionPrivate(sessionId, !isPrivate)
          .then((result) => {
            if (result.status === "error") {
              toast.error(t`Couldn't change Private`, {
                description: result.error,
              });
            }
          });
      }}
      disabled={session == null}
      className="cursor-pointer"
    >
      {isPrivate ? <Eye /> : <EyeSlash />}
      <span>
        {isPrivate ? (
          <Trans>Make Not Private</Trans>
        ) : (
          <Trans>Make Private</Trans>
        )}
      </span>
    </DropdownMenuItem>
  );
}

export function PrivateIndicator({ sessionId }: { sessionId: string }) {
  const session = useSession(sessionId);
  if (session?.private !== true) return null;

  return (
    <span
      data-private-indicator
      title="Private: left out of all-meetings Ask, bulk exports and sharing"
      className="text-muted-foreground inline-flex items-center gap-1 text-xs"
    >
      <EyeSlash className="size-3.5" />
      <Trans>Private</Trans>
    </span>
  );
}
