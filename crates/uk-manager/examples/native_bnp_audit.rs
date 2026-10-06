//! Read a BNP using an existing settings file and export into an explicit audit
//! directory. This never changes the user's installed mods or deploys to Cemu.
use anyhow_ext::{Context, Result};
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};
use uk_manager::{bnp::register_native_reader, mods::Mod, settings::Settings};
use uk_mod::unpack::{ModReader, ModUnpacker};

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow_ext::ensure!(
        args.len() == 3,
        "Usage: native_bnp_audit <settings.yml> <mod.bnp> <audit-output>"
    );
    let settings_path = PathBuf::from(&args[0]);
    let bnp = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    let mut settings = Settings::read(&settings_path)?;
    let dump = settings.dump().context("No game dump configured")?;
    let platform = settings.current_mode;
    let language = settings
        .platform_config()
        .context("No platform settings")?
        .language;
    settings.storage_dir = output.join("isolated-storage");
    let settings = Arc::new(RwLock::new(settings));
    register_native_reader(&settings);
    println!("Opening native BNP: {}", bnp.display());
    let reader = ModReader::open(&bnp, vec![])?;
    anyhow_ext::ensure!(reader.path == bnp, "Native reader changed the source path");
    let mut mod_ = Mod::from_reader(reader);
    mod_.enable_default_options();
    println!(
        "Default options: {:?}",
        mod_.enabled_options
            .iter()
            .map(|o| &o.name)
            .collect::<Vec<_>>()
    );
    let reader = ModReader::open(&bnp, mod_.enabled_options.clone())?;
    let manifest = reader.manifest.clone();
    let overrides = reader.rstb_overrides();
    println!(
        "Files: {} base, {} DLC; RSTB declarations: {}",
        manifest.content_files.len(),
        manifest.aoc_files.len(),
        overrides.len()
    );
    std::fs::write(
        output.join("manifest.yml"),
        serde_yaml::to_string(&manifest)?,
    )?;
    let merged = output.join("merged");
    let updates = ModUnpacker::new(
        dump,
        platform.into(),
        language,
        vec![reader],
        merged.clone(),
    )
    .unpack()?;
    std::fs::write(
        output.join("rstb-updates.json"),
        serde_json::to_vec_pretty(&updates)?,
    )?;
    // Match the manager's final RSTB step so the audit export is self-contained.
    let mut table = rstb::ResourceSizeTable::new_from_stock(platform.into());
    for entry in updates.iter() {
        match *entry.value() {
            Some(size) => {
                if table.get(entry.key().as_str()).is_none_or(|old| old < size) {
                    table.set(entry.key().as_str(), size);
                }
            }
            None => {
                table.remove(entry.key().as_str());
            }
        }
    }
    let table_path = merged
        .join(uk_content::platform_content(platform.into()))
        .join("System/Resource/ResourceSizeTable.product.srsizetable");
    std::fs::create_dir_all(table_path.parent().context("RSTB path has no parent")?)?;
    std::fs::write(
        table_path,
        roead::yaz0::compress(table.to_binary(platform.into())),
    )?;
    std::fs::write(
        output.join("summary.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "name": mod_.meta.name, "source": bnp, "language": language,
            "options": mod_.enabled_options.iter().map(|o| &o.name).collect::<Vec<_>>(),
            "base_files": manifest.content_files.len(), "dlc_files": manifest.aoc_files.len(),
            "rstb_declarations": overrides.len(), "rstb_updates": updates.len(), "export": merged,
        }))?,
    )?;
    println!("Native BNP export complete: {}", merged.display());
    Ok(())
}
