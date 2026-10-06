use anyhow_ext::{Context, Result};
use fs_err as fs;
use roead::byml::Byml;
use uk_content::{prelude::Resource, resource::SaveDataPack};

use super::BnpConverter;

impl BnpConverter {
    pub fn handle_savedata(&self) -> Result<()> {
        let save_path = self.current_root.join("logs/savedata.yml");
        if save_path.exists() {
            log::debug!("Processing savedata log");
            let mut diff = Byml::from_text(fs::read_to_string(save_path)?)?.into_map()?;
            let base =
                self.get_from_master_sarc("Pack/Bootup.pack//GameData/savedataformat.ssarc")?;
            let mut base = SaveDataPack::from_binary(base)
                .context("Could not parse save data while reading BNP flags")?;
            {
                if let Some(data) = base.0.get_mut("game_data.sav") {
                    if let Some(add) = diff.remove("add") {
                        let flags = add
                            .as_array()?
                            .iter()
                            .map(|flag| {
                                flag.try_into()
                                    .map_err(anyhow_ext::Error::from)
                                    .with_context(|| format!("Invalid BNP save data flag {flag:?}"))
                            })
                            .collect::<Result<Vec<_>>>()?;
                        data.flags.extend(flags);
                    }
                    if let Some(del) = diff.remove("del") {
                        for hash in del.into_array()?.into_iter() {
                            if let Some(flag) = data
                                .flags
                                .iter_full_mut()
                                .find(|f| f.0.hash == hash.as_i32().unwrap_or(0))
                            {
                                *flag.1 = true;
                            }
                        }
                        data.flags.delete();
                    }
                }
                self.inject_into_sarc(
                    "Pack/Bootup.pack//GameData/savedataformat.ssarc",
                    base.into_binary(self.platform.into()),
                    false,
                )?;
            }
        }
        Ok(())
    }
}
