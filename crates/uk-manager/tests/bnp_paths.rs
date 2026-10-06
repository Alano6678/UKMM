use roead::aamp::{Parameter, ParameterIO, ParameterList, ParameterObject};
use uk_manager::bnp::{parse_aamp_diff, AampDiffEntry};

#[test]
fn empty_aamp_file_table_slots_do_not_drop_real_edits() {
    let path = "content/Pack/TitleBG.pack//Actor/Pack/GameROMPlayer.sbactorpack//Actor/GeneralParamList/Player_Link.bgparamlist";
    let edit = ParameterList::new().with_object("Player", ParameterObject::new()
        .with_parameter("BombReloadTime1", Parameter::F32(0.0)));
    let log = ParameterIO::new().with_object("FileTable", ParameterObject::new()
        .with_parameter("File0", Parameter::StringRef("".into()))
        .with_parameter("File1", Parameter::StringRef(path.into())))
        .with_list(path, edit.clone());
    let patches = parse_aamp_diff("FileTable", &log).unwrap();
    assert_eq!(patches.len(), 1);
    let AampDiffEntry::Sarc(pack) = &patches["content/Pack/TitleBG.pack"] else { panic!("Missing pack") };
    let AampDiffEntry::Sarc(actor) = &pack["Actor/Pack/GameROMPlayer.sbactorpack"] else { panic!("Missing actor") };
    let AampDiffEntry::Aamp(actual) = &actor["Actor/GeneralParamList/Player_Link.bgparamlist"] else { panic!("Missing parameter edit") };
    assert_eq!(actual, &edit);
}

#[test]
fn nested_empty_absolute_and_parent_paths_remain_rejected() {
    for path in [
        "content/Pack/TitleBG.pack//",
        "content/Pack/TitleBG.pack//../Actor/test.bgparamlist",
        "content/Pack/TitleBG.pack//..\\Actor\\test.bgparamlist",
        "C:/outside.pack//Actor/test.bgparamlist",
        "/outside.pack//Actor/test.bgparamlist",
    ] {
        let log = ParameterIO::new().with_object("FileTable", ParameterObject::new()
            .with_parameter("File0", Parameter::StringRef(path.into())));
        let error = match parse_aamp_diff("FileTable", &log) {
            Ok(_) => panic!("Invalid path was accepted: {path}"),
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains("Invalid FileTable entry"), "{error}");
        assert!(error.contains("invalid relative path"), "{error}");
        if path.ends_with("//") {
            assert!(error.contains("\"\""), "Empty paths must be visible: {error}");
        }
    }
}
