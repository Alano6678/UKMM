use std::{path::Path, sync::Arc};

use uk_content::constants::Language;
use uk_manager::{core::Manager, settings::{DeployConfig, Platform, PlatformSettings, Settings, UpdatePreference}};
use uk_reader::ResourceReader;

fn fixture(root: &Path) -> Settings {
    let dump = root.join("dump");
    std::fs::create_dir_all(dump.join("content/Model")).unwrap();
    std::fs::write(dump.join("content/Model/Armor_001.sbfres"), [1, 2, 3, 4]).unwrap();
    Settings {
        current_mode: Platform::WiiU,
        storage_dir: root.join("storage"),
        wiiu_config: Some(PlatformSettings {
            language: Language::USen,
            profile: "Default".into(),
            dump: Arc::new(ResourceReader::from_unpacked_mod(dump).unwrap()),
            deploy_config: Some(DeployConfig::default()),
        }),
        ..Default::default()
    }
}

#[test]
fn ordinary_settings_save_keeps_readers_cache_and_profile() {
    let temp = tempfile::tempdir().unwrap();
    let settings = fixture(temp.path());
    let dump = settings.dump().unwrap();
    let resource = dump.get_data("Model/Armor_001.bfres").unwrap();
    let core = Manager::from_settings(settings.clone()).unwrap();
    let mut next = settings;
    next.check_updates = UpdatePreference::None;
    next.system_7z = false;
    next.wiiu_config.as_mut().unwrap().deploy_config.as_mut().unwrap().auto = true;
    let path = temp.path().join("config/settings.yml");
    let update = core.save_settings_to(next, &path).unwrap();
    assert!(!update.refresh_mods && !update.reset_package);
    assert!(Arc::ptr_eq(&dump, &core.settings().dump().unwrap()));
    assert!(Arc::ptr_eq(&resource, &dump.get_data("Model/Armor_001.bfres").unwrap()));
    let saved = Settings::read(&path).unwrap();
    assert_eq!(saved.check_updates, UpdatePreference::None);
    assert!(!saved.system_7z);
    assert!(saved.platform_config().unwrap().deploy_config.as_ref().unwrap().auto);
    assert_eq!(core.mod_manager().path(), temp.path().join("storage/wiiu/profiles/Default"));
}

#[test]
fn profile_storage_language_and_dump_changes_are_applied() {
    let temp = tempfile::tempdir().unwrap();
    let core = Manager::from_settings(fixture(temp.path())).unwrap();
    let path = temp.path().join("settings.yml");
    let mut next = core.settings().clone();
    next.wiiu_config.as_mut().unwrap().profile = "Alternate".into();
    let update = core.save_settings_to(next, &path).unwrap();
    assert!(update.refresh_mods && !update.reset_package);
    assert!(core.mod_manager().path().ends_with("profiles/Alternate"));
    let mut next = core.settings().clone();
    next.storage_dir = temp.path().join("new-storage");
    let update = core.save_settings_to(next, &path).unwrap();
    assert!(update.refresh_mods && update.reset_package);
    assert!(core.mod_manager().path().starts_with(temp.path().join("new-storage")));
    let mut next = core.settings().clone();
    next.wiiu_config.as_mut().unwrap().language = Language::EUen;
    assert!(core.save_settings_to(next, &path).unwrap().reset_package);
    let mut next = core.settings().clone();
    let replacement = fixture(&temp.path().join("replacement"));
    let reader = replacement.dump().unwrap();
    next.wiiu_config.as_mut().unwrap().dump = reader.clone();
    let update = core.save_settings_to(next, &path).unwrap();
    assert!(update.refresh_mods && update.reset_package);
    assert!(Arc::ptr_eq(&reader, &core.settings().dump().unwrap()));
}

#[test]
fn failed_settings_write_does_not_change_runtime_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let settings = fixture(temp.path());
    let reader = settings.dump().unwrap();
    let core = Manager::from_settings(settings.clone()).unwrap();
    let mut next = settings;
    next.storage_dir = temp.path().join("different-storage");
    assert!(core.save_settings_to(next, temp.path()).is_err());
    assert_eq!(core.settings().storage_dir, temp.path().join("storage"));
    assert!(Arc::ptr_eq(&reader, &core.settings().dump().unwrap()));
    assert!(core.mod_manager().path().starts_with(temp.path().join("storage")));
}

#[test]
fn failed_storage_reload_restores_saved_settings_and_existing_managers() {
    let temp = tempfile::tempdir().unwrap();
    let settings = fixture(temp.path());
    let reader = settings.dump().unwrap();
    let core = Manager::from_settings(settings.clone()).unwrap();
    let blocker = temp.path().join("blocked-storage");
    std::fs::write(&blocker, b"regular file").unwrap();
    let mut next = settings;
    next.storage_dir = blocker;
    let path = temp.path().join("settings.yml");
    assert!(core.save_settings_to(next, &path).is_err());
    assert_eq!(Settings::read(&path).unwrap().storage_dir, temp.path().join("storage"));
    assert_eq!(core.settings().storage_dir, temp.path().join("storage"));
    assert!(Arc::ptr_eq(&reader, &core.settings().dump().unwrap()));
    assert_eq!(core.mod_manager().path(), temp.path().join("storage/wiiu/profiles/Default"));
}
