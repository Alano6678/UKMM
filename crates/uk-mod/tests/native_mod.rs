use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use uk_content::{constants::Language, prelude::Endian, resource::ResourceData};
use uk_mod::{
    Manifest, Meta, ModCategory, ModOption, ModPlatform,
    pack::MemoryMod,
    unpack::{ModReader, ModUnpacker},
};
use uk_reader::ResourceReader;

fn memory_mod() -> Arc<MemoryMod> {
    Arc::new(MemoryMod {
        meta: Meta {
            api: env!("CARGO_PKG_VERSION").into(),
            name: "Native fixture".into(),
            version: "1.0.0".into(),
            author: "Test".into(),
            category: ModCategory::Other,
            description: "".into(),
            platform: ModPlatform::Specific(Endian::Big),
            url: None,
            options: vec![],
            masters: Default::default(),
        },
        resources: [
            (
                PathBuf::from("Audit/test.bin"),
                Arc::new(ResourceData::Binary(vec![1, 2, 3, 4])),
            ),
            (
                PathBuf::from("options/extra/Audit/test.bin"),
                Arc::new(ResourceData::Binary(vec![5, 6, 7, 8])),
            ),
        ]
        .into_iter()
        .collect(),
        manifests: ["", "options/extra"]
            .into_iter()
            .map(|root| {
                (
                    PathBuf::from(root),
                    Manifest {
                        content_files: ["Audit/test.bin".into()].into_iter().collect(),
                        aoc_files: Default::default(),
                    },
                )
            })
            .collect(),
        rstb_layers: [
            (
                PathBuf::from(""),
                [
                    ("Audit/test.bin".into(), 100),
                    ("Audit/remove.bin".into(), 0),
                ]
                .into_iter()
                .collect(),
            ),
            (
                PathBuf::from("options/extra"),
                [("Audit/test.bin".into(), 200)].into_iter().collect(),
            ),
        ]
        .into_iter()
        .collect(),
        base_priority: false,
        dependencies: vec![],
    })
}

fn option() -> ModOption {
    ModOption {
        name: "Extra".into(),
        description: "".into(),
        path: "extra".into(),
        requires: vec![],
    }
}

#[test]
fn native_layers_preserve_resources_and_rstb() {
    let base = ModReader::from_memory("fixture.bnp".into(), memory_mod(), vec![]).unwrap();
    assert_eq!(
        base.get_resources(Path::new("Audit/test.bin")).unwrap(),
        vec![ResourceData::Binary(vec![1, 2, 3, 4])]
    );
    assert_eq!(base.rstb_overrides()["Audit/test.bin"], 100);
    let selected =
        ModReader::from_memory("fixture.bnp".into(), memory_mod(), vec![option()]).unwrap();
    assert_eq!(
        selected.get_resources(Path::new("Audit/test.bin")).unwrap(),
        vec![
            ResourceData::Binary(vec![1, 2, 3, 4]),
            ResourceData::Binary(vec![5, 6, 7, 8]),
        ]
    );
    assert_eq!(selected.rstb_overrides()["Audit/test.bin"], 200);
    let first = selected
        .get_shared_resources(Path::new("Audit/test.bin"))
        .unwrap();
    let second = selected
        .get_shared_resources(Path::new("Audit/test.bin"))
        .unwrap();
    assert!(Arc::ptr_eq(&first[0], &second[0]));
    assert!(Arc::ptr_eq(&first[1], &second[1]));
}

#[test]
fn native_export_uses_selected_layer_and_preserves_rstb_removal() {
    let temp = tempfile::tempdir().unwrap();
    let dump_dir = temp.path().join("dump");
    std::fs::create_dir_all(dump_dir.join("content")).unwrap();
    let dump = Arc::new(ResourceReader::from_unpacked_mod(&dump_dir).unwrap());
    let reader =
        ModReader::from_memory("fixture.bnp".into(), memory_mod(), vec![option()]).unwrap();
    let output = temp.path().join("output");
    let rstb = ModUnpacker::new(
        dump,
        Endian::Big,
        Language::USen,
        vec![reader],
        output.clone(),
    )
    .unpack()
    .unwrap();
    assert_eq!(
        std::fs::read(output.join("content/Audit/test.bin")).unwrap(),
        [5, 6, 7, 8]
    );
    assert_eq!(*rstb.get("Audit/test.bin").unwrap(), Some(200));
    assert_eq!(*rstb.get("Audit/remove.bin").unwrap(), None);
}

#[test]
fn native_reader_reopens_original_path_after_serialization() {
    let memory = memory_mod();
    uk_mod::native::register_bnp_opener(move |path, options| {
        ModReader::from_memory(path.to_owned(), memory.clone(), options)
    });
    let original = ModReader::open("fixture.bnp", vec![option()]).unwrap();
    let stored = serde_yaml::to_string(&original).unwrap();
    let reopened: ModReader = serde_yaml::from_str(&stored).unwrap();
    assert_eq!(reopened.path, PathBuf::from("fixture.bnp"));
    assert_eq!(
        reopened.get_resources(Path::new("Audit/test.bin")).unwrap(),
        original.get_resources(Path::new("Audit/test.bin")).unwrap()
    );
}

#[test]
fn native_merge_requires_enabled_dependencies() {
    let temp = tempfile::tempdir().unwrap();
    let dump_dir = temp.path().join("dump");
    std::fs::create_dir_all(dump_dir.join("content")).unwrap();
    let dump = Arc::new(ResourceReader::from_unpacked_mod(&dump_dir).unwrap());
    let mut memory = memory_mod();
    Arc::get_mut(&mut memory)
        .unwrap()
        .dependencies
        .push(("Required mod".into(), "1.0.0".into()));
    let reader = ModReader::from_memory("fixture.bnp".into(), memory, vec![]).unwrap();
    let error = ModUnpacker::new(
        dump,
        Endian::Big,
        Language::USen,
        vec![reader],
        temp.path().join("output"),
    )
    .unpack()
    .unwrap_err();
    assert!(format!("{error:#}").contains("requires enabled mod Required mod"));
}

#[test]
fn zip_dictionary_decompression_is_correct_across_parallel_jobs() {
    use rayon::prelude::*;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let dump_dir = temp.path().join("dump");
    std::fs::create_dir_all(source.join("content/Audit")).unwrap();
    std::fs::create_dir_all(dump_dir.join("content")).unwrap();
    for index in 0..32u8 {
        std::fs::write(
            source.join(format!("content/Audit/file{index}.bin")),
            vec![index; 16384],
        )
        .unwrap();
    }
    let dump = Arc::new(ResourceReader::from_unpacked_mod(&dump_dir).unwrap());
    let zip = temp.path().join("parallel.zip");
    uk_mod::pack::ModPacker::new(&source, &zip, Some(memory_mod().meta.clone()), vec![dump])
        .unwrap()
        .pack()
        .unwrap();
    let reader = ModReader::open(&zip, vec![]).unwrap();
    (0..128u8).into_par_iter().for_each(|index| {
        let value = index % 32;
        let resources = reader
            .get_resources(Path::new(&format!("Audit/file{value}.bin")))
            .unwrap();
        assert_eq!(
            resources[0].as_binary().unwrap(),
            vec![value; 16384].as_slice()
        );
    });
}

#[test]
fn rstb_crc_aliases_follow_mod_order_and_largest_floor() {
    let alias = "System/Resource/BotwPortRstbFloor/a95d4845/00000f00/001/28884de5";
    let actual = "Actor/ModelList/Armor_160_Head.bmodellist";
    assert_eq!(roead::aamp::Name::from(alias).hash(), roead::aamp::Name::from(actual).hash());
    let mut first = Arc::try_unwrap(memory_mod()).unwrap();
    first.rstb_layers.clear();
    first.rstb_layers.insert(PathBuf::new(), [(alias.into(), 3840), (actual.into(), 2048)].into_iter().collect());
    let mut last = Arc::try_unwrap(memory_mod()).unwrap();
    last.rstb_layers.clear();
    last.meta.name = "Deletion".into();
    last.rstb_layers.insert(PathBuf::new(), [(actual.into(), 0)].into_iter().collect());
    let first = ModReader::from_memory("first.bnp".into(), Arc::new(first), vec![]).unwrap();
    let last = ModReader::from_memory("last.bnp".into(), Arc::new(last), vec![]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("dump/content")).unwrap();
    let dump = Arc::new(ResourceReader::from_unpacked_mod(temp.path().join("dump")).unwrap());
    let deleted = ModUnpacker::new(dump.clone(), Endian::Big, Language::USen,
        vec![first.clone(), last.clone()], temp.path().join("deleted")).unpack().unwrap();
    assert_eq!(deleted.get(actual).map(|v| *v), Some(None));
    assert!(!deleted.contains_key(alias));
    let restored = ModUnpacker::new(dump, Endian::Big, Language::USen,
        vec![last, first], temp.path().join("restored")).unpack().unwrap();
    assert_eq!(restored.get(actual).map(|v| *v), Some(Some(3840)));
    assert!(!restored.contains_key(alias));
}

#[test]
fn embedded_models_keep_their_archive_scope() {
    use roead::sarc::{Sarc, SarcWriter};
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let dump_dir = temp.path().join("dump");
    std::fs::create_dir_all(source.join("content/Model")).unwrap();
    std::fs::create_dir_all(source.join("content/Pack")).unwrap();
    std::fs::create_dir_all(dump_dir.join("content")).unwrap();
    let external = [b"FRES".as_slice(), &[1; 28]].concat();
    let first = [b"FRES".as_slice(), &[2; 28]].concat();
    let second = [b"FRES".as_slice(), &[3; 28]].concat();
    std::fs::write(source.join("content/Model/shared.bfres"), &external).unwrap();
    for (name, data) in [("First", &first), ("Second", &second)] {
        let nested = SarcWriter::new(roead::Endian::Big)
            .with_file("Model/shared.bfres", data.clone()).to_binary();
        let packed = SarcWriter::new(roead::Endian::Big)
            .with_file("Pack/Nested.pack", nested).to_binary();
        std::fs::write(source.join(format!("content/Pack/{name}.pack")), packed).unwrap();
    }
    let dump = Arc::new(ResourceReader::from_unpacked_mod(&dump_dir).unwrap());
    let zip = temp.path().join("scoped-models.zip");
    uk_mod::pack::ModPacker::new(&source, &zip, Some(memory_mod().meta.clone()), vec![dump.clone()])
        .unwrap().pack().unwrap();
    let output = temp.path().join("output");
    let updates = ModUnpacker::new(dump, Endian::Big, Language::USen,
        vec![ModReader::open(&zip, vec![]).unwrap()], output.clone()).unpack().unwrap();
    assert!(updates.iter().all(|entry| !entry.key().contains("//")));
    assert_eq!(std::fs::read(output.join("content/Model/shared.bfres")).unwrap(), external);
    for (name, expected) in [("First", first), ("Second", second)] {
        let bytes = std::fs::read(output.join(format!("content/Pack/{name}.pack"))).unwrap();
        let outer = Sarc::new(&bytes).unwrap();
        let inner = Sarc::new(outer.get_data("Pack/Nested.pack").unwrap()).unwrap();
        assert_eq!(inner.get_data("Model/shared.bfres").unwrap(), expected);
    }
}

#[test]
fn rstb_removal_clears_duplicate_hash_and_named_entries() {
    let mut table = rstb::ResourceSizeTable::default();
    let name = "Actor/ModelList/Armor_160_Head.bmodellist";
    table.set(name, 128);
    table.name_map.insert(rstb::FixedString::new(name), 3840);
    uk_content::util::remove_rstb_resource(&mut table, name);
    assert!(!table.contains(name));
    assert!(table.get(name).is_none());
    let restored = rstb::ResourceSizeTable::from_binary(table.to_binary(rstb::Endian::Little)).unwrap();
    assert!(!restored.contains(name));
}

#[test]
fn ai_program_custom_top_level_parameters_survive_merge() {
    use roead::aamp::{Parameter, ParameterIO, ParameterList, ParameterListing, ParameterObject};
    use uk_content::{actor::params::aiprog::AIProgram, prelude::Mergeable};
    let base = ParameterIO::new()
        .with_object("DemoAIActionIdx", ParameterObject::default())
        .with_list("AI", ParameterList::default())
        .with_list("Action", ParameterList::default())
        .with_list("Behavior", ParameterList::default())
        .with_list("Query", ParameterList::default());
    let custom = ParameterObject::new()
        .with_parameter("LongReference", Parameter::StringRef(" ".repeat(1024).into()));
    let extra_list = ParameterList::new()
        .with_object("Custom", ParameterObject::new().with_parameter("Value", Parameter::I32(42)));
    let changed = base.clone().with_object("CustomObject", custom.clone())
        .with_list("CustomList", extra_list.clone());
    let baseline = AIProgram::try_from(&base).unwrap();
    let modified = AIProgram::try_from(&changed).unwrap();
    let result: ParameterIO = baseline.merge(&baseline.diff(&modified)).into();
    assert_eq!(result.object("CustomObject"), Some(&custom));
    assert_eq!(result.list("CustomList"), Some(&extra_list));
    assert_eq!(ParameterIO::from_binary(result.to_binary()).unwrap(), result);
}

#[test]
fn variable_length_actor_parameter_strings_preserve_long_utf8() {
    use roead::aamp::{Parameter, ParameterIO, ParameterObject};
    let value = "装备修复_CustomActor;".repeat(80);
    assert!(value.len() > 598);
    let archive = ParameterIO::new().with_object("General",
        ParameterObject::new().with_parameter("LongReference", Parameter::StringRef(value.clone().into())));
    let data = archive.to_binary();
    let restored = ParameterIO::from_binary(&data).unwrap();
    assert_eq!(restored, archive);
    let offset = data.windows(value.len()).position(|bytes| bytes == value.as_bytes()).unwrap();
    let mut invalid = data.clone();
    invalid[offset] = 255;
    assert!(ParameterIO::from_binary(&invalid).is_err());
    assert!(ParameterIO::from_binary(&data[..offset + value.len()]).is_err());
    let whitespace = ParameterIO::new().with_object("General", ParameterObject::new()
        .with_parameter("LongReference", Parameter::StringRef(" ".repeat(1024).into())));
    assert_eq!(ParameterIO::from_binary(whitespace.to_binary()).unwrap(), whitespace);
}

#[test]
fn native_bnp_and_zip_share_load_order_and_read_errors_are_visible() {
    let temp = tempfile::tempdir().unwrap();
    let dump_dir = temp.path().join("dump");
    let source = temp.path().join("zip-source");
    std::fs::create_dir_all(dump_dir.join("content")).unwrap();
    std::fs::create_dir_all(source.join("content/Audit")).unwrap();
    std::fs::write(source.join("content/Audit/test.bin"), [9, 10, 11, 12]).unwrap();
    let dump = Arc::new(ResourceReader::from_unpacked_mod(&dump_dir).unwrap());
    let zip = temp.path().join("overlay.zip");
    let mut meta = memory_mod().meta.clone();
    meta.name = "ZIP overlay".into();
    uk_mod::pack::ModPacker::new(&source, &zip, Some(meta.clone()), vec![dump.clone()])
        .unwrap()
        .pack()
        .unwrap();
    let native =
        ModReader::from_memory("fixture.bnp".into(), memory_mod(), vec![option()]).unwrap();
    let zipped = ModReader::open(&zip, vec![]).unwrap();
    let output = temp.path().join("output");
    ModUnpacker::new(
        dump.clone(),
        Endian::Big,
        Language::USen,
        vec![native.clone(), zipped.clone()],
        output.clone(),
    )
    .unpack()
    .unwrap();
    assert_eq!(
        std::fs::read(output.join("content/Audit/test.bin")).unwrap(),
        [9, 10, 11, 12]
    );
    ModUnpacker::new(
        dump.clone(),
        Endian::Big,
        Language::USen,
        vec![zipped, native],
        output.clone(),
    )
    .unpack()
    .unwrap();
    assert_eq!(
        std::fs::read(output.join("content/Audit/test.bin")).unwrap(),
        [5, 6, 7, 8]
    );

    // A declared ZIP resource with invalid zstd data must fail, not disappear.
    use std::io::Write;
    let broken = temp.path().join("broken.zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&broken).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let manifest = Manifest {
        content_files: ["Audit/test.bin".into()].into_iter().collect(),
        aoc_files: Default::default(),
    };
    for (name, bytes) in [
        (
            "meta.yml",
            serde_yaml::to_string(&meta).unwrap().into_bytes(),
        ),
        (
            "manifest.yml",
            serde_yaml::to_string(&manifest).unwrap().into_bytes(),
        ),
        ("Audit/test.bin", b"invalid compressed resource".to_vec()),
    ] {
        writer.start_file(name, options).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();
    let reader = ModReader::open(&broken, vec![]).unwrap();
    let error = ModUnpacker::new(dump, Endian::Big, Language::USen, vec![reader], output)
        .unpack()
        .unwrap_err();
    assert!(format!("{error:#}").contains("Failed to read Audit/test.bin"));
}
