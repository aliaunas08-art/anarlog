use std::path::Path;
use tauri_plugin_opener::OpenerExt;

pub struct Opener2<'a, R: tauri::Runtime, M: tauri::Manager<R>> {
    manager: &'a M,
    _runtime: std::marker::PhantomData<fn() -> R>,
}

/// Programs the webview may ask to open a URL or path with (ASPC A3.6).
/// Empty: every caller uses the system default, and a webview-chosen program
/// could turn a written file into code execution.
const OPEN_WITH_ALLOWLIST: &[&str] = &[];

fn checked_open_with(with: Option<&str>) -> crate::Result<Option<&str>> {
    match with {
        None => Ok(None),
        Some(program) if OPEN_WITH_ALLOWLIST.contains(&program) => Ok(Some(program)),
        Some(program) => Err(crate::Error::OpenWithNotAllowed(program.to_string())),
    }
}

impl<'a, R: tauri::Runtime, M: tauri::Manager<R>> Opener2<'a, R, M> {
    pub fn open_url(&self, url: &str, with: Option<&str>) -> crate::Result<()> {
        let with = checked_open_with(with)?;
        self.manager.opener().open_url(url, with)?;
        Ok(())
    }

    pub fn open_path(&self, path: &str, with: Option<&str>) -> crate::Result<()> {
        let with = checked_open_with(with)?;
        self.manager.opener().open_path(path, with)?;
        Ok(())
    }

    pub fn reveal_item_in_dir<P: AsRef<Path>>(&self, path: P) -> crate::Result<()> {
        self.manager.opener().reveal_item_in_dir(path)?;
        Ok(())
    }
}

pub trait Opener2PluginExt<R: tauri::Runtime> {
    fn opener2(&self) -> Opener2<'_, R, Self>
    where
        Self: tauri::Manager<R> + Sized;
}

impl<R: tauri::Runtime, T: tauri::Manager<R>> Opener2PluginExt<R> for T {
    fn opener2(&self) -> Opener2<'_, R, Self>
    where
        Self: Sized,
    {
        Opener2 {
            manager: self,
            _runtime: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn open_with_is_rejected_unless_allowlisted() {
        assert!(matches!(super::checked_open_with(None), Ok(None)));
        assert!(super::checked_open_with(Some("Terminal")).is_err());
        assert!(super::checked_open_with(Some("/bin/sh")).is_err());
    }
}
