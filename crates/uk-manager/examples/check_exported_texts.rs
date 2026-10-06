//! Verify an exported Bootup language pack against explicit BNP text-log layers.
use std::{collections::BTreeMap, path::PathBuf};

use anyhow_ext::{Context, Result};
use uk_content::{message::{Entry, MessagePack}, prelude::Resource};

fn semantic_entry(entry: &Entry) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(entry)?;
    // Empty text fragments contribute no bytes to MSBT and disappear on read.
    if let Some(contents) = value.get_mut("contents").and_then(|v| v.as_array_mut()) {
        contents.retain(|item| !(item.as_object().is_some_and(|obj| obj.len() == 1)
            && item.get("text").and_then(|text| text.as_str()) == Some("")));
    }
    Ok(value)
}

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow_ext::ensure!(args.len() >= 4,
        "Usage: check_exported_texts <Bootup_LANGUAGE.pack> <LANGUAGE> <report.json> <texts.json> ...");
    let language = args[1].to_str().context("Invalid language")?;
    let mut expected = BTreeMap::<String, BTreeMap<String, Entry>>::new();
    for path in &args[3..] {
        let path = PathBuf::from(path);
        if !path.is_file() {
            continue;
        }
        let logs: BTreeMap<String, BTreeMap<String, BTreeMap<String, Entry>>> =
            serde_json::from_slice(&std::fs::read(path)?)?;
        for (file, entries) in logs.get(language).context("Language absent from text log")? {
            expected.entry(file.trim_end_matches(".msyt").into())
                .or_default().extend(entries.clone());
        }
    }
    anyhow_ext::ensure!(!expected.is_empty(), "No text changes checked");
    let data = std::fs::read(&args[0])?;
    let outer = roead::sarc::Sarc::new(&data)?;
    let message = outer.get_data(&format!("Message/Msg_{language}.product.ssarc"))
        .context("Exported language pack has no message archive")?;
    let decompressed = roead::yaz0::decompress(message)?;
    let actual = MessagePack::from_binary(decompressed)?;
    let mut missing = Vec::new();
    let mut different = Vec::new();
    let mut differences = Vec::new();
    let mut normalized_empty_text = 0;
    let mut checked = 0;
    for (file, entries) in &expected {
        for (label, entry) in entries {
            checked += 1;
            match actual.0.get(file.as_str()).and_then(|msyt| msyt.entries.get(label)) {
                None => missing.push(format!("{file}/{label}")),
                Some(value) if value != entry => {
                    if semantic_entry(value)? == semantic_entry(entry)? {
                        normalized_empty_text += 1;
                    } else {
                        different.push(format!("{file}/{label}"));
                        differences.push(serde_json::json!({"path": format!("{file}/{label}"),
                            "expected": entry, "actual": value}));
                    }
                }
                _ => {}
            }
        }
    }
    let report = serde_json::json!({
        "language": language, "files": expected.len(), "checked_entries": checked,
        "missing": missing, "different": different, "differences": differences,
        "normalized_empty_text": normalized_empty_text,
    });
    std::fs::write(&args[2], serde_json::to_vec_pretty(&report)?)?;
    println!("Verified {checked} {language} text entries: {} missing, {} different",
        missing.len(), different.len());
    anyhow_ext::ensure!(missing.is_empty() && different.is_empty(), "Exported text differs from BNP logs");
    Ok(())
}
