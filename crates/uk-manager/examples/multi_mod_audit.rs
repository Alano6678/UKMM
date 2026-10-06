//! Install and merge four real mods in isolated storage, including a native ZIP.
use anyhow_ext::{Context, Result};
use parking_lot::RwLock;
use std::{path::{Path, PathBuf}, sync::Arc, time::Instant};
use uk_manager::{mods::Manager, settings::Settings};
use uk_mod::{pack::ModPacker, unpack::{ModReader, ModUnpacker}};

fn export(settings: &Settings, readers: Vec<ModReader>, destination: &Path) -> Result<serde_json::Value> {
    let start = Instant::now();
    let platform = settings.current_mode;
    let language = settings.platform_config().context("No platform config")?.language;
    let dump = settings.dump().context("No game dump")?;
    let count = readers.len();
    let output = destination.join("merged");
    let updates = ModUnpacker::new(dump, platform.into(), language, readers, output.clone()).unpack()?;
    std::fs::write(destination.join("rstb-updates.json"), serde_json::to_vec_pretty(&updates)?)?;
    let mut table = rstb::ResourceSizeTable::new_from_stock(platform.into());
    for entry in updates.iter() {
        match *entry.value() {
            Some(size) if table.get(entry.key().as_str()).is_none_or(|old| old < size) =>
                table.set(entry.key().as_str(), size),
            None => { uk_content::util::remove_rstb_resource(&mut table, entry.key().as_str()); }
            _ => {}
        }
    }
    let table_path = output.join(uk_content::platform_content(platform.into()))
        .join("System/Resource/ResourceSizeTable.product.srsizetable");
    std::fs::create_dir_all(table_path.parent().context("No RSTB parent")?)?;
    std::fs::write(table_path, roead::yaz0::compress(table.to_binary(platform.into())))?;
    let result = serde_json::json!({"mods": count, "merge_ms": start.elapsed().as_secs_f64()*1000.,
        "rstb_updates": updates.len(), "output": output});
    println!("{}: {}", destination.display(), result);
    Ok(result)
}

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow_ext::ensure!(args.len() == 6 || args.len() == 7,
        "Usage: multi_mod_audit <settings.yml> <output> <SecondWind> <Linkle> <Linkle-addon> <gameplay-mod> [styled]");
    let styled = args.len() == 7;
    let output = PathBuf::from(&args[1]);
    let mut config = Settings::read(Path::new(&args[0]))?;
    config.storage_dir = output.join("storage");
    std::fs::create_dir_all(&output)?;
    let settings = Arc::new(RwLock::new(config));
    let manager = Manager::init(&settings)?;
    let mut installed = Vec::new();
    let mut installations = Vec::new();
    for path in &args[2..6] {
        let start = Instant::now();
        let path = PathBuf::from(path);
        let name = ModReader::open_peek(&path, vec![])?.meta.name;
        let mut item = match manager.mods().find(|m| m.meta.name == name) {
            Some(item) => item,
            None => manager.add(&path, None)?,
        };
        item.enabled_options.clear();
        item.enable_default_options();
        if styled && installed.len() == 2 {
            item.enabled_options.extend(item.meta.options.iter().flat_map(|group| match group {
                uk_mod::OptionGroup::Exclusive(group) => group.options.iter(),
                uk_mod::OptionGroup::Multiple(group) => group.options.iter(),
            }).filter(|option| option.path.ends_with("v1_Makeup_Freckles")
                || option.path.ends_with("v4_Ruby__Glasses_Cyan")).cloned());
            anyhow_ext::ensure!(item.enabled_options.len() == 2, "Styled audit options not found");
        }
        manager.set_enabled_options(item.hash(), item.enabled_options.clone())?;
        installations.push(serde_json::json!({"name": item.meta.name, "source": path,
            "options": item.enabled_options.iter().map(|o| &o.name).collect::<Vec<_>>(),
            "install_ms": start.elapsed().as_secs_f64()*1000.}));
        println!("Installed: {}", item.meta.name);
        installed.push(item);
    }
    manager.save()?;
    drop(manager);
    let manager = Manager::init(&settings)?;
    for item in &mut installed {
        let reopened = manager.get_mod(item.hash()).context("Installed mod lost after restart")?;
        anyhow_ext::ensure!(reopened.enabled_options == item.enabled_options,
            "Default options lost after restart for {}", item.meta.name);
        *item = reopened;
    }
    let config = settings.read().clone();
    let open = |indices: &[usize]| -> Result<Vec<ModReader>> {
        indices.iter().map(|&i| ModReader::open(&installed[i].path, installed[i].enabled_options.clone())).collect()
    };
    let mut cases = Vec::new();
    // Separate exports provide expected replacements and demonstrate each input works alone.
    for i in 1..4 {
        cases.push(export(&config, open(&[i])?, &output.join(format!("solo-{i}")))?);
    }
    cases.push(export(&config, open(&[0, 1, 2, 3])?, &output.join("all-forward"))?);
    cases.push(export(&config, open(&[3, 2, 1, 0])?, &output.join("all-reverse"))?);
    let generated = output.join("generated");
    std::fs::create_dir_all(&generated)?;
    let zip = generated.join("Linkle-defaults.zip");
    let mut meta = installed[1].meta.clone();
    // This archive contains the selected defaults, already flattened into one layer.
    meta.options.clear();
    ModPacker::new(&output.join("solo-1/merged"), &zip, Some(meta),
        vec![config.dump().context("No game dump")?])?.pack()?;
    cases.push(export(&config, vec![ModReader::open(&zip, vec![])?], &output.join("solo-zip"))?);
    let mut mixed = open(&[0])?;
    mixed.push(ModReader::open(&zip, vec![])?);
    mixed.extend(open(&[2, 3])?);
    cases.push(export(&config, mixed, &output.join("mixed-zip"))?);
    std::fs::write(output.join("summary.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "installations": installations, "restart_verified": true, "cases": cases,
    }))?)?;
    Ok(())
}
