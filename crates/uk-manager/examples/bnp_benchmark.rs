//! Repeatable installation and merge benchmark with isolated storage/output.
use anyhow_ext::{Context, Result};
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc, time::Instant};
use uk_manager::{
    mods::{Manager, Mod},
    settings::Settings,
};
use uk_mod::unpack::{ModReader, ModUnpacker};

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow_ext::ensure!(
        args.len() == 3 || args.len() == 4,
        "Usage: bnp_benchmark <settings.yml> <BNP> <output-directory> [warm]"
    );
    let warm = args.len() == 4;
    let settings_path = PathBuf::from(&args[0]);
    let source = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);
    let mut settings = Settings::read(&settings_path)?;
    settings.storage_dir = output.join("storage");
    let dump = settings.dump().context("No game dump configured")?;
    let platform = settings.current_mode;
    let language = settings
        .platform_config()
        .context("No platform config")?
        .language;
    let settings = Arc::new(RwLock::new(settings));
    let start = Instant::now();
    let manager = Manager::init(&settings)?;
    let startup_ms = start.elapsed().as_secs_f64() * 1000.;
    println!("Initialize stored mods: {startup_ms:.2} ms");
    let start = Instant::now();
    let mut opened = Mod::from_reader(ModReader::open_peek(&source, vec![])?);
    let open_ms = start.elapsed().as_secs_f64() * 1000.;
    println!("Open BNP: {open_ms:.2} ms");
    opened.enable_default_options();
    let start = Instant::now();
    let installed = if warm {
        manager
            .mods()
            .find(|m| m.meta.name == opened.meta.name)
            .context("No installed benchmark mod")?
    } else {
        manager.add(&source, None)?
    };
    let install_ms = start.elapsed().as_secs_f64() * 1000.;
    println!("Install (copy + reopen): {install_ms:.2} ms");
    manager.save()?;
    let start = Instant::now();
    let reader = ModReader::open(&installed.path, opened.enabled_options)?;
    let reopen_ms = start.elapsed().as_secs_f64() * 1000.;
    let files = reader.manifest.content_files.len() + reader.manifest.aoc_files.len();
    // Compare merge engines with the same cold ROM caches. BNP decoding warms
    // them in the old build but a persisted native cache does not.
    let dump = Arc::new(serde_yaml::from_str::<uk_reader::ResourceReader>(
        &serde_yaml::to_string(&dump)?,
    )?);
    let start = Instant::now();
    let updates = ModUnpacker::new(
        dump,
        platform.into(),
        language,
        vec![reader],
        output.join("merged"),
    )
    .unpack()?;
    let merge_ms = start.elapsed().as_secs_f64() * 1000.;
    println!("Merge: {merge_ms:.2} ms");
    std::fs::write(
        output.join("rstb-updates.json"),
        serde_json::to_vec_pretty(&updates)?,
    )?;
    let mut table = rstb::ResourceSizeTable::new_from_stock(platform.into());
    for entry in updates.iter() {
        match *entry.value() {
            Some(size) => {
                if table.get(entry.key().as_str()).is_none_or(|old| old < size) {
                    table.set(entry.key().as_str(), size);
                }
            }
            None => {
                uk_content::util::remove_rstb_resource(&mut table, entry.key().as_str());
            }
        }
    }
    let table_path = output
        .join("merged")
        .join(uk_content::platform_content(platform.into()))
        .join("System/Resource/ResourceSizeTable.product.srsizetable");
    std::fs::create_dir_all(table_path.parent().context("RSTB path has no parent")?)?;
    std::fs::write(
        table_path,
        roead::yaz0::compress(table.to_binary(platform.into())),
    )?;
    std::fs::write(
        output.join(if warm {
            "timings-warm.json"
        } else {
            "timings.json"
        }),
        serde_json::to_vec_pretty(&serde_json::json!({
            "open_ms": open_ms, "install_ms": install_ms, "reopen_ms": reopen_ms,
            "merge_ms": merge_ms, "files": files, "rstb_entries": updates.len(), "startup_ms": startup_ms,
        }))?,
    )?;
    Ok(())
}
