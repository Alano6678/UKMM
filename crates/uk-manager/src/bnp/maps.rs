use super::BnpConverter;
use anyhow::anyhow;
use anyhow_ext::{Context, Result};
use fs_err as fs;
use rayon::prelude::*;
use roead::{
    byml::{Byml, Map},
    sarc::{Sarc, SarcWriter},
    yaz0::{compress, decompress},
};
use std::path::Path;

fn merge_map(base: &mut Byml, diff: Byml) -> Result<()> {
    let mut diff = diff.into_map()?;
    let base = base.as_mut_map()?;

    fn merge_section(base: &mut Vec<Byml>, diff: &mut Map) -> Result<()> {
        fn hash_id(object: &Byml) -> Result<u32> {
            Ok(object
                .as_map()?
                .get("HashId")
                .context("Map entry missing HashId")?
                .as_int()?)
        }
        let mut entries = base
            .iter()
            .map(|object| -> Result<_> { Ok((hash_id(object)?, object.clone())) })
            .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
        if let Some(Byml::Map(mods)) = diff.remove("mod") {
            for (hash, entry) in mods {
                let hash: u32 = hash.parse()?;
                if hash_id(&entry)? != hash {
                    anyhow::bail!("BNP map entry HashId does not match its modification key");
                }
                entries.insert(hash, entry);
            }
        }
        if let Some(Byml::Array(dels)) = diff.remove("del") {
            for hash in dels {
                entries.remove(&hash.as_int()?);
            }
        }
        if let Some(Byml::Array(adds)) = diff.remove("add") {
            for object in adds {
                // BNP generation may use a different base/DLC map copy. A
                // matching ID must not discard the author's supplied object.
                entries.insert(hash_id(&object)?, object);
            }
        }
        *base = entries.into_values().collect();
        Ok(())
    }

    if let (Some(Byml::Map(mut diff_objs)), Some(Byml::Array(base_objs))) =
        (diff.remove("Objs"), base.get_mut("Objs"))
    {
        merge_section(base_objs, &mut diff_objs)?;
    }
    if let (Some(Byml::Map(mut diff_rails)), Some(Byml::Array(base_rails))) =
        (diff.remove("Rails"), base.get_mut("Rails"))
    {
        merge_section(base_rails, &mut diff_rails)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bnp_map_coordinates_preserve_float_precision() {
        let value = Byml::from_text("value: -3224.42627").unwrap();
        assert_eq!(
            value.as_map().unwrap()["value"]
                .as_float()
                .unwrap()
                .to_bits(),
            "-3224.42627".parse::<f32>().unwrap().to_bits()
        );
    }

    #[test]
    fn bnp_map_preserves_changes_when_ids_exist_in_another_baseline() {
        let mut base =
            Byml::from_text("Objs: [{HashId: !u 1, UnitConfigName: Old}]\nRails: []").unwrap();
        let diff = Byml::from_text("Objs:\n  add: [{HashId: !u 1, UnitConfigName: New}]\n  mod: {'2': {HashId: !u 2, UnitConfigName: Added}}\n  del: [!u 3]\nRails: {}\n").unwrap();
        merge_map(&mut base, diff).unwrap();
        let objects = base.as_map().unwrap()["Objs"].as_array().unwrap();
        assert_eq!(objects.len(), 2);
        assert_eq!(
            objects[0].as_map().unwrap()["UnitConfigName"]
                .as_string()
                .unwrap(),
            "New"
        );
        assert_eq!(
            objects[1].as_map().unwrap()["UnitConfigName"]
                .as_string()
                .unwrap(),
            "Added"
        );
    }
}

impl BnpConverter {
    // Pull out all map files in a temp dir for handle_maps
    // There's an insanely complicated set of circumstances
    // for managing where the files should come from, for
    // all the different combinations of circumstances, so
    // sort that all out here and just have handle_maps use
    // the state we've built.
    pub fn set_up_temp_map_state(&self) -> Result<()> {
        let maps_path = self.current_root.join("logs/map.yml");
        if !maps_path.exists() {
            return Ok(());
        }

        let is_root = self.current_root == self.path;
        let maps = if is_root {
            self.root_maps.clone()
        } else {
            self.opt_maps.clone()
        };
        let (has_aoc_dump, dump_static_pack) = if let Ok(aoc_main_field) = self
            .dump
            .get_aoc_bytes_uncached("Aoc/0010/Pack/AocMainField.pack")
        {
            (
                true,
                Sarc::new(aoc_main_field).context("Could not read Pack/AocMainField.pack")?,
            )
        } else {
            (
                false,
                Sarc::new(
                    self.dump
                        .get_bytes_uncached("Pack/TitleBG.pack")
                        .context("Could not find map archive in dump")?,
                )
                .context("Could not read Pack/TitleBG.pack")?,
            )
        };
        let diff = Byml::from_text(fs::read_to_string(maps_path)?)
            .context("Could not parse maps log")?
            .into_map()?;
        diff.into_par_iter()
            .map(|(hash, _)| -> Result<()> {
                let (section, load_type) = hash.split_once('_').ok_or(anyhow!("Bad map diff"))?;
                let canon = format!("Map/MainField/{section}/{section}_{load_type}.smubin");
                if !is_root && let Some(root_data) = self.root_maps.get(&canon) {
                    maps.insert(canon, root_data.value().clone());
                } else {
                    maps.insert(
                        canon.clone(),
                        match (load_type, has_aoc_dump) {
                            ("Dynamic", true) => self
                                .dump
                                .get_aoc_bytes_uncached(Path::new("Aoc/0010").join(&canon))
                                .with_context(|| format!("Could not find Aoc map file {canon}"))?,
                            ("Dynamic", false) => self
                                .dump
                                .get_bytes_uncached(&canon)
                                .with_context(|| format!("Could not find base map file {canon}"))?,
                            ("Static", _) => dump_static_pack
                                .get(&canon)
                                .with_context(|| format!("Failed to extract {canon}"))?
                                .data
                                .into(),
                            _ => anyhow::bail!("Invalid hash: {hash}"),
                        },
                    );
                }
                Ok(())
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    pub fn handle_maps(&self) -> Result<()> {
        let maps_path = self.current_root.join("logs/map.yml");
        if !maps_path.exists() {
            return Ok(());
        }

        log::debug!("Processing maps log");
        let maps = if self.current_root == self.path {
            self.root_maps.clone()
        } else {
            self.opt_maps.clone()
        };
        Byml::from_text(fs::read_to_string(maps_path)?)
            .context("Could not parse maps log")?
            .into_map()?
            .into_par_iter()
            .map(|(section, diff)| -> Result<()> {
                let square = section.split_once('_').ok_or(anyhow!("Bad map diff"))?.0;
                let canon = format!("Map/MainField/{square}/{section}.smubin");
                let mut base = Byml::from_binary(
                    decompress(maps.get(&canon).expect("Lost a map file?").value())
                        .with_context(|| format!("Failed to decompress map {canon}"))?,
                )
                .with_context(|| format!("Failed to read map {canon}"))?;
                merge_map(&mut base, diff).with_context(|| {
                    if !self
                        .dump
                        .source()
                        .file_exists(Path::new("Pack/AocMainField.pack"))
                    {
                        format!(
                            "Failed to rebuild {section}. This map section *may* contain \
                                edits to DLC data, and your DLC path is not set."
                        )
                    } else {
                        format!("Failed to rebuild {section}.")
                    }
                })?;
                maps.insert(canon, compress(base.to_binary(self.platform.into())));
                Ok(())
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    pub fn clear_temp_map_state(&self) -> Result<()> {
        let maps_path = self.current_root.join("logs/map.yml");
        if !maps_path.exists() {
            return Ok(());
        }

        let has_aoc_dump = self
            .dump
            .source()
            .file_exists(Path::new("Pack/AocMainField.pack"));
        let dest_path = self
            .current_root
            .join(if has_aoc_dump { self.aoc } else { self.content })
            .join(if has_aoc_dump {
                "Pack/AocMainField.pack"
            } else {
                "Pack/TitleBG.pack"
            });
        let root_pack = if let Ok(bytes) = std::fs::read(&dest_path) {
            Sarc::new(bytes).ok()
        } else {
            None
        };
        let dump_pack = if has_aoc_dump {
            Sarc::new(self.get_master_aoc_bytes("Pack/AocMainField.pack")?)?
        } else {
            Sarc::new(self.get_master_bytes("Pack/TitleBG.pack")?)?
        };
        let mut merged_pack = root_pack
            .map(|pack| {
                let mut ret = SarcWriter::from_sarc(&pack);
                for file in dump_pack.files() {
                    if file.name.is_some_and(|name| !ret.files.contains_key(name)) {
                        ret.add_file(file.unwrap_name(), file.data);
                    }
                }
                ret
            })
            .unwrap_or_else(|| SarcWriter::from_sarc(&dump_pack));
        let dynamic_prefix = if has_aoc_dump {
            self.current_root.join(self.aoc)
        } else {
            self.current_root.join(self.content)
        };

        let maps = if self.current_root == self.path {
            self.root_maps.clone()
        } else {
            self.opt_maps.clone()
        };
        for map in maps.iter() {
            let canon = map.key();
            if canon.ends_with("_Static.smubin") {
                merged_pack.add_file(canon, map.value().clone());
            } else {
                let out = dynamic_prefix.join(canon);
                out.parent().iter().try_for_each(fs::create_dir_all)?;
                std::fs::write(out, map.value())?;
            }
        }

        dest_path.parent().iter().try_for_each(fs::create_dir_all)?;
        fs::write(dest_path, merged_pack.to_binary())?;

        if self.current_root != self.path {
            maps.clear();
        }
        Ok(())
    }
}
