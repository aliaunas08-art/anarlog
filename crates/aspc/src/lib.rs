//! Central access check for ASPC (docs/ASPC_BASE.md §0, A1, A2, A4).
//!
//! Everything here is a pure function of its inputs so the same crate can run
//! in the desktop app and, later, on a server. The only place that compares
//! roles is [`authorize`] (A1.2, enforced by `tests/role_comparisons.rs`).

pub mod commands;
pub mod lock;
mod scope;
mod state;
#[cfg(feature = "tauri")]
mod tauri_guard;

pub use lock::{AuthResult, LockMachine, LockSnapshot, UnlockOutcome};
pub use scope::WorkspaceScope;
pub use state::AspcState;
#[cfg(feature = "tauri")]
pub use tauri_guard::{CallerLabel, require, require_ignoring_lock};

/// A1.1. The roles, defined once. Database values are parsed with
/// [`Role::parse`]; an unknown value is `None` and every check denies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Role {
    Owner,
    Admin,
    Member,
    Viewer,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Owner, Role::Admin, Role::Member, Role::Viewer];

    /// Parses a stored role name (`workspace_memberships.role`, hosted
    /// `role` column). Anything else, including different case, is unknown.
    pub fn parse(value: &str) -> Option<Role> {
        match value {
            "owner" => Some(Role::Owner),
            "admin" => Some(Role::Admin),
            "member" => Some(Role::Member),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Viewer => "viewer",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum DataType {
    Recording,
    Transcript,
    Notes,
    Actions,
    Settings,
    /// Billing and API keys, tokens, webhook secrets.
    Keys,
}

impl DataType {
    pub const ALL: [DataType; 6] = [
        DataType::Recording,
        DataType::Transcript,
        DataType::Notes,
        DataType::Actions,
        DataType::Settings,
        DataType::Keys,
    ];

    pub fn is_content(self) -> bool {
        matches!(
            self,
            DataType::Recording | DataType::Transcript | DataType::Notes | DataType::Actions
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Action {
    Create,
    Read,
    Edit,
    Export,
    Share,
    Delete,
    Administer,
}

impl Action {
    pub const ALL: [Action; 7] = [
        Action::Create,
        Action::Read,
        Action::Edit,
        Action::Export,
        Action::Share,
        Action::Delete,
        Action::Administer,
    ];
}

/// Who is acting, where, and whether the app lock has been passed (§0).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct AccessContext {
    /// Local mode: the id the data model writes as `owner_user_id` (the
    /// CloudSync binding's local workspace id, or the account id once
    /// claimed; see `anlg_db_app::local_content_identity`).
    pub actor_id: String,
    /// `None` means the stored role was not one of the four: denied.
    pub role: Option<Role>,
    pub workspace_id: String,
    pub device_id: String,
    /// The calling window's label. `None` for checks that are not tied to a
    /// window (the webview SQL authorizer; the window is checked at the
    /// command level there).
    pub window: Option<String>,
    pub unlocked: bool,
}

/// The resource owner, needed only for `Member` on content (A1 matrix).
///
/// Command-level checks don't know which row they will touch and pass
/// [`Ownership::Unknown`]; for a `Member` that allows only `Create` (the new
/// row will be theirs). Row-level owner checks come with workspace filtering
/// (Phase 2b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership<'a> {
    Unknown,
    /// `owner_user_id` of the row.
    OwnedBy(&'a str),
    /// The row belongs to someone else and was shared to the actor.
    SharedToActor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantScope {
    Full,
    /// Admin on Keys: may learn whether a key is set, never its value.
    PresenceOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    pub data: DataType,
    pub action: Action,
    pub scope: GrantScope,
}

impl Grant {
    /// Commands that return or change a secret value need a full grant.
    pub fn require_full(self) -> Result<Grant, Denied> {
        match self.scope {
            GrantScope::Full => Ok(self),
            GrantScope::PresenceOnly => {
                Err(Denied::new(DenyCode::PresenceOnly, self.data, self.action))
            }
        }
    }
}

/// Stable denial codes. The string form is part of the error contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DenyCode {
    Locked,
    WrongWorkspace,
    UnknownRole,
    RoleDenied,
    NotOwner,
    WindowDenied,
    PresenceOnly,
    NoContext,
}

impl DenyCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DenyCode::Locked => "aspc.locked",
            DenyCode::WrongWorkspace => "aspc.wrong_workspace",
            DenyCode::UnknownRole => "aspc.unknown_role",
            DenyCode::RoleDenied => "aspc.role_denied",
            DenyCode::NotOwner => "aspc.not_owner",
            DenyCode::WindowDenied => "aspc.window_denied",
            DenyCode::PresenceOnly => "aspc.presence_only",
            DenyCode::NoContext => "aspc.no_context",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Denied {
    pub code: DenyCode,
    pub data: DataType,
    pub action: Action,
}

impl Denied {
    pub fn new(code: DenyCode, data: DataType, action: Action) -> Self {
        Self { code, data, action }
    }
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: not authorized ({:?} {:?})",
            self.code.as_str(),
            self.action,
            self.data
        )
    }
}

impl std::error::Error for Denied {}

impl From<Denied> for String {
    fn from(value: Denied) -> Self {
        value.to_string()
    }
}

/// The central check (§0) for content whose owner is not known at this
/// point. Same as [`authorize_resource`] with [`Ownership::Unknown`].
pub fn authorize(
    ctx: &AccessContext,
    workspace_id: &str,
    data: DataType,
    action: Action,
) -> Result<Grant, Denied> {
    authorize_resource(ctx, workspace_id, data, action, Ownership::Unknown)
}

/// The central check with the resource owner, for `Member` rules.
///
/// Order: lock, workspace, role, matrix, window. Every denial carries a
/// stable [`DenyCode`].
pub fn authorize_resource(
    ctx: &AccessContext,
    workspace_id: &str,
    data: DataType,
    action: Action,
    owner: Ownership<'_>,
) -> Result<Grant, Denied> {
    let deny = |code| Err(Denied::new(code, data, action));

    if !ctx.unlocked {
        return deny(DenyCode::Locked);
    }
    if ctx.workspace_id.is_empty() || workspace_id != ctx.workspace_id {
        return deny(DenyCode::WrongWorkspace);
    }
    let Some(role) = ctx.role else {
        return deny(DenyCode::UnknownRole);
    };

    let scope = match matrix(role, data, action, &ctx.actor_id, owner) {
        Ok(scope) => scope,
        Err(code) => return deny(code),
    };

    if let Some(window) = ctx.window.as_deref()
        && !window_allows(WindowKind::from_label(window), data, action)
    {
        return deny(DenyCode::WindowDenied);
    }

    Ok(Grant {
        data,
        action,
        scope,
    })
}

/// The A1 matrix, exactly as in ASPC_BASE.
fn matrix(
    role: Role,
    data: DataType,
    action: Action,
    actor_id: &str,
    owner: Ownership<'_>,
) -> Result<GrantScope, DenyCode> {
    use Action::*;
    use DataType::*;

    let allowed = |ok: bool| {
        if ok {
            Ok(GrantScope::Full)
        } else {
            Err(DenyCode::RoleDenied)
        }
    };

    match role {
        Role::Owner => Ok(GrantScope::Full),
        Role::Admin => match data {
            Settings => allowed(matches!(action, Read | Edit | Administer)),
            Keys => {
                if action == Read {
                    Ok(GrantScope::PresenceOnly)
                } else {
                    Err(DenyCode::RoleDenied)
                }
            }
            _ => Ok(GrantScope::Full),
        },
        Role::Member => match data {
            Settings => allowed(action == Read),
            Keys => Err(DenyCode::RoleDenied),
            _ => {
                if action == Administer {
                    return Err(DenyCode::RoleDenied);
                }
                let own = matches!(owner, Ownership::OwnedBy(id) if !actor_id.is_empty() && id == actor_id);
                match (action, owner) {
                    (Create, Ownership::Unknown) => Ok(GrantScope::Full),
                    (_, _) if own => Ok(GrantScope::Full),
                    (Read, Ownership::SharedToActor) => Ok(GrantScope::Full),
                    _ => Err(DenyCode::NotOwner),
                }
            }
        },
        Role::Viewer => allowed(data.is_content() && action == Read),
    }
}

/// Window kinds, from the label. Unknown labels get nothing (fail closed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Main,
    Note,
    Composer,
    /// Floating bar and live caption: display surfaces of the active meeting.
    Overlay,
    DictationOverlay,
    Unknown,
}

impl WindowKind {
    pub fn from_label(label: &str) -> WindowKind {
        match label {
            "main" => WindowKind::Main,
            "composer" => WindowKind::Composer,
            "floating-bar" | "live-caption" => WindowKind::Overlay,
            "dictation-overlay" => WindowKind::DictationOverlay,
            _ if label.strip_prefix("note-").is_some_and(|id| !id.is_empty()) => WindowKind::Note,
            _ => WindowKind::Unknown,
        }
    }
}

/// Per-window limits (A2.5). Only `main` and `note-*` touch Keys or Export;
/// the composer gets content and settings but no Keys, Export, Share,
/// Delete or Administer; overlays read the meeting and settings only.
pub fn window_allows(window: WindowKind, data: DataType, action: Action) -> bool {
    use Action::*;
    match window {
        WindowKind::Main | WindowKind::Note => true,
        WindowKind::Composer => match data {
            DataType::Keys => false,
            DataType::Settings => matches!(action, Read | Edit),
            _ => matches!(action, Create | Read | Edit),
        },
        WindowKind::Overlay => match data {
            DataType::Keys | DataType::Actions => false,
            _ => action == Read,
        },
        WindowKind::DictationOverlay | WindowKind::Unknown => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WS: &str = "ws-1";
    const ME: &str = "actor-1";

    fn ctx(role: Option<Role>) -> AccessContext {
        AccessContext {
            actor_id: ME.into(),
            role,
            workspace_id: WS.into(),
            device_id: "device".into(),
            window: None,
            unlocked: true,
        }
    }

    fn ok(role: Role, data: DataType, action: Action) -> bool {
        authorize(&ctx(Some(role)), WS, data, action).is_ok()
    }

    #[test]
    fn owner_may_do_everything() {
        for data in DataType::ALL {
            for action in Action::ALL {
                let grant = authorize(&ctx(Some(Role::Owner)), WS, data, action).unwrap();
                assert_eq!(grant.scope, GrantScope::Full);
            }
        }
    }

    #[test]
    fn admin_matrix() {
        use Action::*;
        for data in DataType::ALL.into_iter().filter(|d| d.is_content()) {
            for action in Action::ALL {
                assert!(ok(Role::Admin, data, action), "{data:?} {action:?}");
            }
        }
        for action in Action::ALL {
            assert_eq!(
                ok(Role::Admin, DataType::Settings, action),
                matches!(action, Read | Edit | Administer),
                "{action:?}"
            );
        }
        let read = authorize(&ctx(Some(Role::Admin)), WS, DataType::Keys, Read).unwrap();
        assert_eq!(read.scope, GrantScope::PresenceOnly);
        assert_eq!(
            read.require_full().unwrap_err().code,
            DenyCode::PresenceOnly
        );
        for action in Action::ALL.into_iter().filter(|a| *a != Read) {
            assert!(!ok(Role::Admin, DataType::Keys, action), "{action:?}");
        }
    }

    #[test]
    fn member_matrix() {
        use Action::*;
        let c = ctx(Some(Role::Member));
        let table: &[(Action, Ownership<'_>, bool)] = &[
            (Create, Ownership::Unknown, true),
            (Read, Ownership::Unknown, false),
            (Edit, Ownership::Unknown, false),
            (Create, Ownership::OwnedBy(ME), true),
            (Read, Ownership::OwnedBy(ME), true),
            (Edit, Ownership::OwnedBy(ME), true),
            (Export, Ownership::OwnedBy(ME), true),
            (Share, Ownership::OwnedBy(ME), true),
            (Delete, Ownership::OwnedBy(ME), true),
            (Administer, Ownership::OwnedBy(ME), false),
            (Read, Ownership::OwnedBy("someone-else"), false),
            (Edit, Ownership::OwnedBy("someone-else"), false),
            (Delete, Ownership::OwnedBy("someone-else"), false),
            (Read, Ownership::SharedToActor, true),
            (Edit, Ownership::SharedToActor, false),
            (Export, Ownership::SharedToActor, false),
            (Delete, Ownership::SharedToActor, false),
        ];
        for data in DataType::ALL.into_iter().filter(|d| d.is_content()) {
            for (action, owner, expected) in table {
                assert_eq!(
                    authorize_resource(&c, WS, data, *action, *owner).is_ok(),
                    *expected,
                    "{data:?} {action:?} {owner:?}"
                );
            }
        }
        for action in Action::ALL {
            assert_eq!(ok(Role::Member, DataType::Settings, action), action == Read);
            assert!(!ok(Role::Member, DataType::Keys, action));
        }
        let mut anonymous = c.clone();
        anonymous.actor_id.clear();
        assert!(
            authorize_resource(
                &anonymous,
                WS,
                DataType::Notes,
                Read,
                Ownership::OwnedBy("")
            )
            .is_err()
        );
    }

    /// A1.4.
    #[test]
    fn viewer_reads_content_only() {
        for data in DataType::ALL {
            for action in Action::ALL {
                let expected = action == Action::Read && data.is_content();
                assert_eq!(
                    ok(Role::Viewer, data, action),
                    expected,
                    "{data:?} {action:?}"
                );
            }
        }
        for action in [
            Action::Edit,
            Action::Delete,
            Action::Export,
            Action::Share,
            Action::Administer,
        ] {
            for data in DataType::ALL {
                let denied = authorize(&ctx(Some(Role::Viewer)), WS, data, action).unwrap_err();
                assert_eq!(denied.code, DenyCode::RoleDenied);
            }
        }
    }

    #[test]
    fn lock_workspace_and_unknown_role_deny() {
        let mut locked = ctx(Some(Role::Owner));
        locked.unlocked = false;
        assert_eq!(
            authorize(&locked, WS, DataType::Notes, Action::Read)
                .unwrap_err()
                .code,
            DenyCode::Locked
        );
        assert_eq!(
            authorize(
                &ctx(Some(Role::Owner)),
                "ws-2",
                DataType::Notes,
                Action::Read
            )
            .unwrap_err()
            .code,
            DenyCode::WrongWorkspace
        );
        let mut empty = ctx(Some(Role::Owner));
        empty.workspace_id.clear();
        assert_eq!(
            authorize(&empty, "", DataType::Notes, Action::Read)
                .unwrap_err()
                .code,
            DenyCode::WrongWorkspace
        );
        assert_eq!(
            authorize(&ctx(None), WS, DataType::Notes, Action::Read)
                .unwrap_err()
                .code,
            DenyCode::UnknownRole
        );
        assert_eq!(Role::parse("Owner"), None);
        assert_eq!(Role::parse("editor"), None);
        for role in Role::ALL {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
    }

    #[test]
    fn windows_limit_owner() {
        let with_window = |label: &str| {
            let mut c = ctx(Some(Role::Owner));
            c.window = Some(label.into());
            c
        };
        let cases: &[(&str, DataType, Action, bool)] = &[
            ("main", DataType::Keys, Action::Read, true),
            ("note-abc", DataType::Keys, Action::Edit, true),
            ("note-abc", DataType::Notes, Action::Export, true),
            ("note-", DataType::Notes, Action::Read, false),
            ("composer", DataType::Keys, Action::Read, false),
            ("composer", DataType::Notes, Action::Export, false),
            ("composer", DataType::Notes, Action::Delete, false),
            ("composer", DataType::Notes, Action::Edit, true),
            ("composer", DataType::Settings, Action::Read, true),
            ("floating-bar", DataType::Settings, Action::Read, true),
            ("live-caption", DataType::Transcript, Action::Read, true),
            ("live-caption", DataType::Notes, Action::Edit, false),
            ("floating-bar", DataType::Keys, Action::Read, false),
            ("dictation-overlay", DataType::Settings, Action::Read, false),
            ("devtools", DataType::Notes, Action::Read, false),
        ];
        for (label, data, action, expected) in cases {
            let result = authorize(&with_window(label), WS, *data, *action);
            assert_eq!(result.is_ok(), *expected, "{label} {data:?} {action:?}");
            if !expected {
                assert_eq!(result.unwrap_err().code, DenyCode::WindowDenied);
            }
        }
    }

    #[test]
    fn denial_codes_are_stable() {
        let denied = Denied::new(DenyCode::Locked, DataType::Keys, Action::Read);
        assert_eq!(
            denied.to_string(),
            "aspc.locked: not authorized (Read Keys)"
        );
    }
}
