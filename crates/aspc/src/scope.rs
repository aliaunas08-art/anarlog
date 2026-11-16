//! C1.2: the workspace a read of content is limited to.

use crate::AccessContext;

/// The workspace every Rust read of content is filtered by (ASPC C1.2).
///
/// Built only from a non-empty workspace id, so a scope can never match
/// "everything": a caller without a workspace gets `None` and must fail
/// closed.
///
/// `includes_unassigned` is set for the device's local workspace. Rows whose
/// `workspace_id` is still `''` belong to that workspace by the data model's
/// own rule (CloudSync's claim moves `''` rows into the local workspace, and
/// the startup backfill gives them the local id). Scopes for any other
/// workspace never match `''`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceScope {
    workspace_id: String,
    includes_unassigned: bool,
}

impl WorkspaceScope {
    /// A scope for exactly `workspace_id`. `None` when it is empty.
    pub fn new(workspace_id: impl Into<String>) -> Option<Self> {
        let workspace_id = workspace_id.into();
        if workspace_id.trim().is_empty() {
            return None;
        }
        Some(Self {
            workspace_id,
            includes_unassigned: false,
        })
    }

    /// The device's local workspace: `workspace_id` plus rows not yet
    /// assigned (`''`). `None` when the id is empty.
    pub fn local(workspace_id: impl Into<String>) -> Option<Self> {
        Self::new(workspace_id).map(|scope| Self {
            includes_unassigned: true,
            ..scope
        })
    }

    /// The local scope of an access context. `None` (fail closed) when the
    /// context has no workspace.
    pub fn from_context(ctx: &AccessContext) -> Option<Self> {
        Self::local(ctx.workspace_id.clone())
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn includes_unassigned(&self) -> bool {
        self.includes_unassigned
    }

    /// Whether a row with this `workspace_id` is inside the scope.
    pub fn contains(&self, workspace_id: &str) -> bool {
        workspace_id == self.workspace_id || (self.includes_unassigned && workspace_id.is_empty())
    }

    /// The two values for a `workspace_id IN (?, ?)` filter. For a scope
    /// without unassigned rows both are the workspace id.
    pub fn sql_values(&self) -> [&str; 2] {
        [
            self.workspace_id.as_str(),
            if self.includes_unassigned {
                ""
            } else {
                self.workspace_id.as_str()
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_workspace_has_no_scope() {
        assert!(WorkspaceScope::new("").is_none());
        assert!(WorkspaceScope::new("  ").is_none());
        assert!(WorkspaceScope::local("").is_none());
    }

    #[test]
    fn local_scope_includes_unassigned_rows_only() {
        let local = WorkspaceScope::local("w1").unwrap();
        assert!(local.contains("w1"));
        assert!(local.contains(""));
        assert!(!local.contains("w2"));
        assert_eq!(local.sql_values(), ["w1", ""]);

        let exact = WorkspaceScope::new("w1").unwrap();
        assert!(exact.contains("w1"));
        assert!(!exact.contains(""));
        assert_eq!(exact.sql_values(), ["w1", "w1"]);
    }
}
