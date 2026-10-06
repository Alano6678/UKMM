use base64::Engine;
use parking_lot::RwLock;
use std::{path::Path, sync::Arc};
use uk_content::{constants::Language, prelude::Endian};
use uk_manager::{
    mods::Manager,
    settings::{Platform, PlatformSettings, Settings},
};
use uk_reader::ResourceReader;

#[test]
fn installing_and_restarting_keeps_the_original_bnp_and_options() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let dump_dir = temp.path().join("dump");
    std::fs::create_dir_all(source.join("content/Audit")).unwrap();
    std::fs::create_dir_all(source.join("logs")).unwrap();
    std::fs::create_dir_all(source.join("options/extra/content/Audit")).unwrap();
    std::fs::create_dir_all(dump_dir.join("content")).unwrap();
    std::fs::write(source.join("content/Audit/native.bin"), b"base resource").unwrap();
    std::fs::write(
        source.join("options/extra/content/Audit/native.bin"),
        b"option resource",
    )
    .unwrap();
    std::fs::write(source.join("logs/packs.json"), "{}").unwrap();
    std::fs::write(source.join("info.json"), serde_json::to_vec(&serde_json::json!({
        "name": "Native BNP fixture", "version": "1.0.0", "desc": "", "platform": "wiiu",
        "options": {"multi": [{"name": "Extra", "desc": "", "folder": "extra", "default": true}], "single": []}
    })).unwrap()).unwrap();
    let bnp = temp.path().join("fixture.bnp");
    sevenz_rust::compress_to_path(&source, &bnp).unwrap();
    let original = std::fs::read(&bnp).unwrap();
    let settings = Arc::new(RwLock::new(Settings {
        storage_dir: temp.path().join("storage"),
        current_mode: Platform::WiiU,
        wiiu_config: Some(PlatformSettings {
            language: Language::USen,
            profile: "Default".into(),
            dump: Arc::new(ResourceReader::from_unpacked_mod(&dump_dir).unwrap()),
            deploy_config: None,
        }),
        ..Settings::default()
    }));
    let manager = Manager::init(&settings).unwrap();
    let mut mod_ = manager.add(&bnp, None).unwrap();
    mod_.enable_default_options();
    let hash = mod_.hash();
    let stored_path = mod_.path.clone();
    manager.profile().mods_mut().insert(hash, mod_);
    manager.save().unwrap();
    assert_eq!(stored_path.extension().unwrap(), "bnp");
    assert_eq!(std::fs::read(&stored_path).unwrap(), original);
    // Installing a BNP with an absent dependency must fail before storage.
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("info.json")).unwrap()).unwrap();
    metadata["name"] = "Dependent native BNP".into();
    metadata["depends"] = serde_json::json!([
        base64::engine::general_purpose::STANDARD.encode("Native foundation==1.0.0")
    ]);
    std::fs::write(
        source.join("info.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    let dependent = temp.path().join("dependent.bnp");
    sevenz_rust::compress_to_path(&source, &dependent).unwrap();
    assert!(
        format!("{:#}", manager.add(&dependent, None).unwrap_err())
            .contains("requires installed mod Native foundation")
    );
    metadata["name"] = "Native foundation".into();
    metadata["depends"] = serde_json::json!([]);
    metadata["priority"] = "base".into();
    std::fs::write(
        source.join("info.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    let foundation = temp.path().join("foundation.bnp");
    sevenz_rust::compress_to_path(&source, &foundation).unwrap();
    manager.add(&foundation, None).unwrap();
    assert_eq!(
        manager.mods().next().unwrap().meta.name,
        "Native foundation"
    );
    manager.add(&dependent, None).unwrap();
    manager.save().unwrap();
    drop(manager);
    let reopened = Manager::init(&settings).unwrap();
    let decoded_cache = settings.read().platform_dir().join("cache/bnp");
    let cache_files = std::fs::read_dir(&decoded_cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    assert!(!cache_files.is_empty());
    let restored = reopened
        .mods()
        .find(|m| m.meta.name == "Native BNP fixture")
        .unwrap();
    assert_eq!(restored.path, stored_path);
    assert_eq!(restored.enabled_options[0].path, Path::new("extra"));
    let first_manifest = restored.manifest().unwrap();
    let repeated_manifest = restored.manifest().unwrap();
    assert!(Arc::ptr_eq(&first_manifest, &repeated_manifest));
    uk_manager::bnp::register_native_reader(&settings);
    let reloaded_manifest = restored.manifest().unwrap();
    assert!(!Arc::ptr_eq(&first_manifest, &reloaded_manifest));
    let reader = uk_mod::unpack::ModReader::open(&restored.path, restored.enabled_options).unwrap();
    assert_eq!(
        reader
            .get_resources(Path::new("Audit/native.bin"))
            .unwrap()
            .last()
            .unwrap()
            .as_binary()
            .unwrap(),
        b"option resource"
    );
    assert_eq!(settings.read().current_mode, Platform::WiiU);
    assert_eq!(Endian::from(settings.read().current_mode), Endian::Big);
    // Cached resources must survive a new reader registration. Corrupt caches
    // are disposable: rebuilding still preserves the original selected layer.
    drop(reopened);
    for file in cache_files {
        std::fs::write(file, b"damaged cache").unwrap();
    }
    let restored = Manager::init(&settings)
        .unwrap()
        .mods()
        .find(|m| m.meta.name == "Native BNP fixture")
        .unwrap();
    let rebuilt =
        uk_mod::unpack::ModReader::open(&restored.path, restored.enabled_options).unwrap();
    assert_eq!(
        rebuilt
            .get_resources(Path::new("Audit/native.bin"))
            .unwrap()
            .last()
            .unwrap()
            .as_binary()
            .unwrap(),
        b"option resource"
    );
    // Unsupported logs cannot result in a success message with missing changes.
    std::fs::write(source.join("logs/unsupported.yml"), "changes: true").unwrap();
    let error = uk_mod::unpack::ModReader::open(&source, vec![]).unwrap_err();
    assert!(format!("{error:#}").contains("Unsupported BNP log"));
}
