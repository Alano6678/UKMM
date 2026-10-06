use anyhow_ext::{Context, Result};
use fs_err as fs;
use roead::byml::Byml;
use uk_content::{
    actor::residents::ResidentActorData, prelude::Resource, resource::ResidentActors,
};

use super::BnpConverter;

impl BnpConverter {
    pub fn handle_residents(&self) -> Result<()> {
        let residents_path = self.current_root.join("logs/residents.yml");
        if residents_path.exists() {
            log::debug!("Processing resident actors log");
            let diff = Byml::from_text(fs::read_to_string(residents_path)?)?.into_map()?;
            let data = self.get_from_master_sarc("Pack/Bootup.pack//Actor/ResidentActors.byml")?;
            let mut residents = ResidentActors::from_binary(data)
                .context("Could not parse resident actors while reading BNP log")?;
            {
                for (name, data) in diff {
                    let actor = ResidentActorData::try_from(data.as_map()?)
                        .with_context(|| format!("Invalid BNP resident actor {name}"))?;
                    residents.0.insert(name, actor);
                }
                self.inject_into_sarc(
                    "Pack/Bootup.pack//Actor/ResidentActors.byml",
                    residents.into_binary(self.platform.into()),
                    false,
                )?;
            }
        }
        Ok(())
    }
}
