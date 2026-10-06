use roead::byml::Byml;
use serde::{Deserialize, Serialize};

use crate::{Result, UKError, prelude::*, util::SortedDeleteMap};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]

pub struct MapUnit {
    pub pos_x: Option<f32>,
    pub pos_z: Option<f32>,
    pub size: Option<f32>,
    pub objects: SortedDeleteMap<u32, Byml>,
    pub rails: SortedDeleteMap<u32, Byml>,
}

impl PartialEq for MapUnit {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &SortedDeleteMap<u32, Byml>, b: &SortedDeleteMap<u32, Byml>| {
            a.iter_full().count() == b.iter_full().count()
                && a.iter_full().zip(b.iter_full()).all(
                    |((key, (value, deleted)), (other_key, (v, d)))| {
                        key == other_key && deleted == d && crate::util::byml_exact_eq(value, v)
                    },
                )
        };
        self.pos_x.map(f32::to_bits) == other.pos_x.map(f32::to_bits)
            && self.pos_z.map(f32::to_bits) == other.pos_z.map(f32::to_bits)
            && self.size.map(f32::to_bits) == other.size.map(f32::to_bits)
            && same(&self.objects, &other.objects)
            && same(&self.rails, &other.rails)
    }
}

impl TryFrom<&Byml> for MapUnit {
    type Error = UKError;

    fn try_from(byml: &Byml) -> Result<Self> {
        let hash = byml.as_map()?;
        Ok(Self {
            pos_x: hash
                .get("LocationPosX")
                .map(|v| -> Result<f32> { Ok(v.as_float()?) })
                .transpose()?,
            pos_z: hash
                .get("LocationPosZ")
                .map(|v| -> Result<f32> { Ok(v.as_float()?) })
                .transpose()?,
            size: hash
                .get("LocationSize")
                .map(|v| -> Result<f32> { Ok(v.as_float()?) })
                .transpose()?,
            objects: hash
                .get("Objs")
                .ok_or(UKError::MissingBymlKey("Map unit missing objs"))?
                .as_array()?
                .iter()
                .map(|obj| -> Result<(u32, Byml)> {
                    let hash = obj.as_map()?;
                    let id = hash
                        .get("HashId")
                        .ok_or(UKError::MissingBymlKey("Map unit object missing hash ID"))?
                        .as_int()?;
                    Ok((id, obj.clone()))
                })
                .collect::<Result<_>>()?,
            rails: hash
                .get("Rails")
                .ok_or(UKError::MissingBymlKey("Map unit missing rails"))?
                .as_array()?
                .iter()
                .map(|obj| -> Result<(u32, Byml)> {
                    let hash = obj.as_map()?;
                    let id = hash
                        .get("HashId")
                        .ok_or(UKError::MissingBymlKey("Map unit rail missing hash ID"))?
                        .as_int()?;
                    Ok((id, obj.clone()))
                })
                .collect::<Result<_>>()?,
        })
    }
}

impl From<MapUnit> for Byml {
    fn from(val: MapUnit) -> Self {
        [
            (
                "Objs",
                val.objects.into_iter().map(|(_, obj)| obj).collect(),
            ),
            ("Rails", val.rails.into_iter().map(|(_, obj)| obj).collect()),
        ]
        .into_iter()
        .chain(
            [
                ("LocationPosX", val.pos_x),
                ("LocationPosZ", val.pos_z),
                ("LocationSize", val.size),
            ]
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, Byml::Float(v)))),
        )
        .collect()
    }
}

impl Mergeable for MapUnit {
    fn diff(&self, other: &Self) -> Self {
        let exact_diff = |base: &SortedDeleteMap<u32, Byml>,
                          changed: &SortedDeleteMap<u32, Byml>| {
            changed
                .iter()
                .filter(|(key, value)| {
                    base.get(*key)
                        .is_none_or(|old| !crate::util::byml_exact_eq(old, value))
                })
                .map(|(key, value)| (*key, value.clone(), false))
                .chain(
                    base.iter()
                        .filter(|(key, _)| !changed.contains_key(*key))
                        .map(|(key, value)| (*key, value.clone(), true)),
                )
                .collect()
        };
        Self {
            pos_x: other.pos_x,
            pos_z: other.pos_z,
            size: other.size,
            objects: exact_diff(&self.objects, &other.objects),
            rails: exact_diff(&self.rails, &other.rails),
        }
    }

    fn merge(&self, diff: &Self) -> Self {
        Self {
            pos_x: diff.pos_x,
            pos_z: diff.pos_z,
            size: diff.size,
            objects: self.objects.merge(&diff.objects),
            rails: self.rails.merge(&diff.rails),
        }
    }
}

impl Resource for MapUnit {
    fn from_binary(data: impl AsRef<[u8]>) -> crate::Result<Self> {
        (&Byml::from_binary(data.as_ref())?).try_into()
    }

    fn into_binary(self, endian: crate::prelude::Endian) -> Vec<u8> {
        Byml::from(self).to_binary(endian.into())
    }

    fn path_matches(path: impl AsRef<std::path::Path>) -> bool {
        path.as_ref()
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                (name.starts_with("CDungeon")
                    || name.contains("Dynamic")
                    || name.contains("_Static"))
                    && name.ends_with("mubin")
            })
            .unwrap_or(false)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use roead::byml::Byml;

    use crate::prelude::*;

    #[test]
    fn small_coordinate_edits_survive_diff_merge_and_binary_export() {
        let original = Byml::from_text(
            "Objs: [{HashId: !u 1, Translate: [-3224.45142, 349.960266, -1330.14307]}]\nRails: []",
        )
        .unwrap();
        let modified = Byml::from_text(
            "Objs: [{HashId: !u 1, Translate: [-3224.42627, 349.960266, -1330.13025]}]\nRails: []",
        )
        .unwrap();
        let base = super::MapUnit::try_from(&original).unwrap();
        let changed = super::MapUnit::try_from(&modified).unwrap();
        assert_ne!(base, changed);
        let diff = base.diff(&changed);
        assert_eq!(diff.objects.len(), 1);
        let result = base.merge(&diff);
        let exported = super::MapUnit::from_binary(result.into_binary(Endian::Big)).unwrap();
        assert_eq!(exported, changed);
        // Even a one-bit rotation edit must remain an explicit patch.
        let mut rotated = changed.clone();
        let mut item = rotated.objects.get(&1).unwrap().clone();
        item.as_mut_map()
            .unwrap()
            .insert("Rotate".into(), Byml::Float(f32::from_bits(0x3f800001)));
        rotated.objects.insert(1u32, item);
        assert_eq!(changed.diff(&rotated).objects.len(), 1);
    }

    fn load_cdungeon_munt() -> Byml {
        Byml::from_binary(
            roead::yaz0::decompress(
                std::fs::read("test/Map/CDungeon/Dungeon044/Dungeon044_Static.smubin").unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn load_mod_cdungeon_munt() -> Byml {
        Byml::from_binary(
            roead::yaz0::decompress(
                std::fs::read("test/Map/CDungeon/Dungeon044/Dungeon044_Static.mod.smubin").unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn load_mainfield_munt() -> Byml {
        Byml::from_binary(
            roead::yaz0::decompress(
                std::fs::read("test/Map/MainField/D-3/D-3_Dynamic.smubin").unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn load_mod_mainfield_munt() -> Byml {
        Byml::from_binary(
            roead::yaz0::decompress(
                std::fs::read("test/Map/MainField/D-3/D-3_Dynamic.mod.smubin").unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn serde_mainfield() {
        let byml = load_mainfield_munt();
        let munt = super::MapUnit::try_from(&byml).unwrap();
        let data = Byml::from(munt.clone()).to_binary(roead::Endian::Big);
        let byml2 = Byml::from_binary(data).unwrap();
        let munt2 = super::MapUnit::try_from(&byml2).unwrap();
        assert_eq!(munt, munt2);
    }

    #[test]
    fn serde_cdungeon() {
        let byml = load_cdungeon_munt();
        let munt = super::MapUnit::try_from(&byml).unwrap();
        let data = Byml::from(munt.clone()).to_binary(roead::Endian::Big);
        let byml2 = Byml::from_binary(data).unwrap();
        let munt2 = super::MapUnit::try_from(&byml2).unwrap();
        assert_eq!(munt, munt2);
    }

    #[test]
    fn diff_mainfield() {
        let byml = load_mainfield_munt();
        let munt = super::MapUnit::try_from(&byml).unwrap();
        let byml2 = load_mod_mainfield_munt();
        let munt2 = super::MapUnit::try_from(&byml2).unwrap();
        let _diff = munt.diff(&munt2);
    }

    #[test]
    fn diff_cdungeon() {
        let byml = load_cdungeon_munt();
        let munt = super::MapUnit::try_from(&byml).unwrap();
        let byml2 = load_mod_cdungeon_munt();
        let munt2 = super::MapUnit::try_from(&byml2).unwrap();
        let _diff = munt.diff(&munt2);
    }

    #[test]
    fn merge_mainfield() {
        let byml = load_mainfield_munt();
        let munt = super::MapUnit::try_from(&byml).unwrap();
        let byml2 = load_mod_mainfield_munt();
        let munt2 = super::MapUnit::try_from(&byml2).unwrap();
        let diff = munt.diff(&munt2);
        let merged = munt.merge(&diff);
        assert_eq!(merged, munt2);
    }

    #[test]
    fn merge_cdungeon() {
        let byml = load_cdungeon_munt();
        let munt = super::MapUnit::try_from(&byml).unwrap();
        let byml2 = load_cdungeon_munt();
        let munt2 = super::MapUnit::try_from(&byml2).unwrap();
        let diff = munt.diff(&munt2);
        let merged = munt.merge(&diff);
        assert_eq!(merged, munt2);
    }

    #[test]
    fn identify() {
        let path = std::path::Path::new("content/Map/MainField/F-3/F-3_Dynamic.smubin");
        assert!(super::MapUnit::path_matches(path));
        let path2 =
            std::path::Path::new("aoc/0010/Map/CDungeon/Dungeon044/Dungeon044_Static.mubin");
        assert!(super::MapUnit::path_matches(path2));
    }
}
