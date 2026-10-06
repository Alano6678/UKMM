//! Native archive readers are supplied by the manager, which owns game settings.
use std::{
    path::Path,
    sync::{Arc, LazyLock},
};

use anyhow_ext::{Context, Result};
use parking_lot::RwLock;

use crate::{ModOption, unpack::ModReader};

type BnpOpener = dyn Fn(&Path, Vec<ModOption>) -> Result<ModReader> + Send + Sync;
static BNP_OPENER: LazyLock<RwLock<Option<Arc<BnpOpener>>>> = LazyLock::new(|| RwLock::new(None));
static BNP_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn generation() -> u64 {
    BNP_GENERATION.load(std::sync::atomic::Ordering::Acquire)
}

pub fn is_bnp(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("bnp"))
        || (path.is_dir() && path.join("info.json").is_file() && path.join("logs").is_dir())
}

pub fn register_bnp_opener(
    opener: impl Fn(&Path, Vec<ModOption>) -> Result<ModReader> + Send + Sync + 'static,
) {
    *BNP_OPENER.write() = Some(Arc::new(opener));
    BNP_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Release);
}

pub(crate) fn open_bnp(path: &Path, options: Vec<ModOption>) -> Result<ModReader> {
    // Release the registry lock before the opener reads settings or resources.
    let opener = BNP_OPENER
        .read()
        .clone()
        .context("Native BNP reader is not configured; open this mod through UKMM")?;
    opener(path, options)
}
