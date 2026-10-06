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
                ResourceData::Binary(vec![1, 2, 3, 4]),
            ),
            (
                PathBuf::from("options/extra/Audit/test.bin"),
                ResourceData::Binary(vec![5, 6, 7, 8]),
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
