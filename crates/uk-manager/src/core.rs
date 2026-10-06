use std::{path::Path, sync::Arc};

use anyhow_ext::{Context, Result};
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{deploy, mods, settings::Settings};

#[derive(Debug, Clone)]
pub struct Manager {
    mod_manager: Arc<RwLock<mods::Manager>>,
    deploy_manager: Arc<RwLock<deploy::Manager>>,
    settings: Arc<RwLock<Settings>>,
}

impl std::panic::RefUnwindSafe for Manager {}

#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsUpdate {
    pub refresh_mods: bool,
    pub reset_package: bool,
}

impl Manager {
    pub fn init() -> Result<Self> {
        let settings = Settings::load();
        Self::from_shared_settings(settings)
    }

    /// Construct a manager for an explicitly supplied configuration.
    pub fn from_settings(settings: Settings) -> Result<Self> {
        Self::from_shared_settings(Arc::new(RwLock::new(settings)))
    }

    fn from_shared_settings(settings: Arc<RwLock<Settings>>) -> Result<Self> {
        let mod_manager = Arc::new(RwLock::new(
            mods::Manager::init(&settings).context("Failed to initialize mod manager")?,
        ));
        Ok(Self {
            deploy_manager: Arc::new(RwLock::new(
                deploy::Manager::init(&settings, &mod_manager)
                    .context("Failed to initialize deployment manager")?,
            )),
            mod_manager,
            settings,
        })
    }

    pub fn reload(&self) -> Result<()> {
        self.settings.write().reload();
        *self.mod_manager.write() =
            mods::Manager::init(&self.settings).context("Failed to initialize mod manager")?;
        *self.deploy_manager.write() = deploy::Manager::init(&self.settings, &self.mod_manager)
            .context("Failed to initialize deployment manager")?;
        Ok(())
    }

    pub fn save_settings(&self, next: Settings) -> Result<SettingsUpdate> {
        self.save_settings_to(next, Settings::path())
    }

    /// Apply already loaded settings instead of deserializing and indexing both
    /// game dumps again. Ordinary preference changes retain readers and mods.
    pub fn save_settings_to(&self, next: Settings, path: &Path) -> Result<SettingsUpdate> {
        let previous = self.settings().clone();
        let reload_managers = previous.storage_dir != next.storage_dir
            || previous.current_mode != next.current_mode;
        let old_profile = previous.platform_config().map(|p| &p.profile);
        let new_profile = next.platform_config().map(|p| &p.profile);
        let change_profile = old_profile != new_profile;
        let change_reader = match (previous.dump(), next.dump()) {
            (Some(old), Some(new)) => !Arc::ptr_eq(&old, &new),
            (None, None) => false,
            _ => true,
        };
        let change_language = previous.platform_config().map(|p| p.language)
            != next.platform_config().map(|p| p.language);
        let update = SettingsUpdate {
            refresh_mods: reload_managers || change_profile || change_reader || change_language,
            reset_package: reload_managers || change_reader || change_language,
        };
        next.save_to(path).context("Failed to save settings")?;
        *self.settings.write() = next;
        let applied = (|| -> Result<()> {
            if reload_managers {
                let mods = mods::Manager::init(&self.settings)
                    .context("Failed to initialize mod manager")?;
                let deploy = deploy::Manager::init(&self.settings, &self.mod_manager)
                    .context("Failed to initialize deployment manager")?;
                *self.mod_manager.write() = mods;
                *self.deploy_manager.write() = deploy;
            } else {
                if change_profile {
                    let profile = self.settings().platform_config()
                        .map(|p| p.profile.clone()).unwrap_or_else(|| "Default".into());
                    self.mod_manager.write().set_profile(profile.as_str())?;
                }
                if change_reader || change_language {
                    crate::bnp::register_native_reader(&self.settings);
                }
            }
            Ok(())
        })();
        if let Err(error) = applied {
            *self.settings.write() = previous.clone();
            if let Err(rollback) = previous.save_to(path) {
                log::error!("Failed to restore settings after reload error: {rollback:#}");
            }
            // Retain existing mods and unsaved edits when a new configuration
            // cannot load, and restore the profile if switching it failed.
            crate::bnp::register_native_reader(&self.settings);
            if change_profile && !reload_managers {
                let old = previous.platform_config().map(|p| p.profile.as_str()).unwrap_or("Default");
                if let Err(rollback) = self.mod_manager.write().set_profile(old) {
                    log::error!("Failed to restore profile after settings error: {rollback:#}");
                }
            }
            return Err(error);
        }
        Ok(update)
    }

    pub fn change_profile(&self, profile: impl AsRef<str>) -> Result<()> {
        self.mod_manager.write().set_profile(profile.as_ref())?;
        if let Some(config) = self.settings.write().platform_config_mut() {
            config.profile = profile.as_ref().into();
        }
        Ok(())
    }

    #[inline(always)]
    pub fn settings(&self) -> RwLockReadGuard<'_, Settings> {
        self.settings.read()
    }

    #[inline(always)]
    pub fn settings_mut(&self) -> RwLockWriteGuard<'_, Settings> {
        self.settings.write()
    }

    #[inline(always)]
    pub fn mod_manager(&self) -> RwLockReadGuard<'_, mods::Manager> {
        self.mod_manager.read()
    }

    #[inline(always)]
    pub fn mod_manager_mut(&self) -> RwLockWriteGuard<'_, mods::Manager> {
        self.mod_manager.write()
    }

    #[inline(always)]
    pub fn deploy_manager(&self) -> RwLockReadGuard<'_, deploy::Manager> {
        self.deploy_manager.read()
    }
}
