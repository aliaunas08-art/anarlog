use std::path::{Path, PathBuf};

use tauri_plugin_settings::SettingsPluginExt;

pub struct Fs2<'a, R: tauri::Runtime, M: tauri::Manager<R>> {
    manager: &'a M,
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<'a, R: tauri::Runtime, M: tauri::Manager<R>> Fs2<'a, R, M> {
    fn base(&self) -> Result<PathBuf, crate::Error> {
        self.manager
            .settings()
            .vault_base()
            .map(|p| p.into_std_path_buf())
            .map_err(|e| crate::Error::Path(e.to_string()))
    }

    /// The app's own roots: the vault and the app data directory, canonical.
    fn roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Ok(base) = self.base() {
            roots.push(base.canonicalize().unwrap_or(base));
        }
        if let Ok(data) = self.manager.path().app_data_dir() {
            roots.push(data.canonicalize().unwrap_or(data));
        }
        roots
    }

    fn validate_path(&self, path: &Path) -> Result<PathBuf, crate::Error> {
        let base = self.base()?;
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        };
        let resolved = crate::grants::resolve(&path)?;
        let canonical_base = base.canonicalize().unwrap_or(base);
        if resolved.starts_with(&canonical_base) {
            Ok(resolved)
        } else {
            Err(crate::Error::PathForbidden(path))
        }
    }

    /// A path the webview may write: inside the vault or app data directory
    /// with a document extension, or one the user just picked in
    /// `pick_save_path` (single use).
    pub fn authorize_write(&self, path: &Path) -> Result<PathBuf, crate::Error> {
        let resolved = crate::grants::resolve(path)?;
        let grants = self.manager.state::<crate::grants::PathGrants>();
        if grants.consume(crate::grants::GrantKind::Write, &resolved) {
            return Ok(resolved);
        }
        if crate::grants::is_under_any(&resolved, &self.roots())
            && crate::grants::has_extension(&resolved, crate::grants::ROOT_WRITE_EXTENSIONS)
        {
            return Ok(resolved);
        }
        Err(crate::Error::PathForbidden(path.to_path_buf()))
    }

    /// A path the webview may read from: inside the vault or app data
    /// directory, or one the user just picked in `pick_open_paths` (single use).
    pub fn authorize_read(&self, path: &Path) -> Result<PathBuf, crate::Error> {
        let resolved = crate::grants::resolve(path)?;
        let grants = self.manager.state::<crate::grants::PathGrants>();
        if grants.consume(crate::grants::GrantKind::Read, &resolved)
            || crate::grants::is_under_any(&resolved, &self.roots())
        {
            return Ok(resolved);
        }
        Err(crate::Error::PathForbidden(path.to_path_buf()))
    }

    pub fn write_text_file(&self, path: &Path, content: &str) -> Result<(), crate::Error> {
        let path = self.authorize_write(path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn read_text_file(&self, path: &Path) -> Result<String, crate::Error> {
        let validated_path = self.validate_path(path)?;
        let content = std::fs::read_to_string(&validated_path)?;
        Ok(content)
    }

    pub fn remove(&self, path: &Path) -> Result<(), crate::Error> {
        let validated_path = self.validate_path(path)?;

        if !validated_path.exists() {
            return Ok(());
        }

        let metadata = std::fs::symlink_metadata(&validated_path)?;

        if metadata.is_dir() {
            std::fs::remove_dir_all(&validated_path)?;
        } else {
            std::fs::remove_file(&validated_path)?;
        }

        Ok(())
    }
}

pub trait Fs2PluginExt<R: tauri::Runtime> {
    fn fs2(&self) -> Fs2<'_, R, Self>
    where
        Self: tauri::Manager<R> + Sized;
}

impl<R: tauri::Runtime, T: tauri::Manager<R>> Fs2PluginExt<R> for T {
    fn fs2(&self) -> Fs2<'_, R, Self>
    where
        Self: Sized,
    {
        Fs2 {
            manager: self,
            _runtime: std::marker::PhantomData,
        }
    }
}
