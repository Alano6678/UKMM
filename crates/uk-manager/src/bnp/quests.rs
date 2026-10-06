use anyhow_ext::{Context, Result};
use fs_err as fs;
use roead::{byml::Byml, yaz0::compress};
use rustc_hash::FxHashMap;
use smartstring::alias::String;

use super::BnpConverter;

impl BnpConverter {
    pub fn handle_quests(&self) -> Result<()> {
        let quests_path = self.current_root.join("logs/quests.yml");
        if quests_path.exists() {
            log::debug!("Processing quests log");
            let mut diff = Byml::from_text(fs::read_to_string(quests_path)?)?.into_map()?;
            let mut quests = Byml::from_binary(
                self.get_from_master_sarc("Pack/TitleBG.pack//Quest/QuestProduct.sbquestpack")?,
            )?
            .into_array()?;
            let quest_hashes: FxHashMap<String, usize> = quests
                .iter()
                .enumerate()
                .filter_map(|(i, quest)| {
                    quest.as_map().ok().and_then(|h| {
                        h.get("Name")
                            .and_then(|n| n.as_string().ok().cloned().map(|n| (n, i)))
                    })
                })
                .collect();
            if let Some(Byml::Map(mods)) = diff.remove("mod") {
                for (name, quest) in mods {
                    let index = quest_hashes
                        .get(&name)
                        .copied()
                        .with_context(|| format!("BNP modifies missing quest {name}"))?;
                    quests[index] = quest;
                }
            }
            if let Some(Byml::Array(dels)) = diff.remove("del") {
                let deleted = dels
                    .iter()
                    .map(Byml::as_string)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                quests.retain(|quest| {
                    quest
                        .as_map()
                        .ok()
                        .and_then(|q| q.get("Name"))
                        .and_then(|n| n.as_string().ok())
                        .is_some_and(|n| !deleted.contains(&n))
                });
            }
            if let Some(Byml::Array(add)) = diff.remove("add") {
                quests.extend(add);
            }
            self.inject_into_sarc(
                "Pack/TitleBG.pack//Quest/QuestProduct.sbquestpack",
                compress(Byml::Array(quests).to_binary(self.platform.into())),
                false,
            )?;
        }
        Ok(())
    }
}
