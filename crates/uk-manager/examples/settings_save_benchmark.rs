//! Measure the legacy full reload and the selective save with isolated paths.
use anyhow_ext::{Context, Result};
use std::{path::PathBuf, time::Instant};
use uk_manager::{core::Manager, settings::Settings};

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow_ext::ensure!(args.len() == 2 || args.len() == 3,
        "Usage: settings_save_benchmark <settings.yml> <output> [isolated-test-storage]");
    let output = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&output)?;
    let mut config = Settings::read(&PathBuf::from(&args[0]))?;
    // Default to new isolated storage; an existing test fixture can be supplied.
    config.storage_dir = args.get(2).map(PathBuf::from).unwrap_or_else(|| output.join("storage"));
    let path = output.join("settings.yml");
    let core = Manager::from_settings(config.clone())?;
    let mut next = config;
    let deploy = next.platform_config_mut().context("No platform settings")?
        .deploy_config.as_mut().context("No deployment configuration")?;
    deploy.auto = !deploy.auto;
    let start = Instant::now();
    let update = core.save_settings_to(next.clone(), &path)?;
    let fast_ms = start.elapsed().as_secs_f64()*1000.;
    anyhow_ext::ensure!(!update.refresh_mods && !update.reset_package, "Preference change reloaded data");
    let start = Instant::now();
    next.save_to(&path)?;
    let reloaded = Settings::read(&path)?;
    let full = Manager::from_settings(reloaded)?;
    let full_ms = start.elapsed().as_secs_f64()*1000.;
    let report = serde_json::json!({"fast_save_ms":fast_ms,"legacy_full_reload_ms":full_ms,
        "ordinary_preference_reloaded_mods":update.refresh_mods,
        "profile_mods":full.mod_manager().mods().count(), "settings_path":path});
    std::fs::write(output.join("benchmark.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}
