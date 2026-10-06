//! Disposable, versioned native BNP cache. The original archive remains intact.
use anyhow_ext::{Context, Result};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use uk_content::{resource::ResourceData, util::HashMap};
use uk_mod::{Manifest, Meta, pack::MemoryMod};
use uk_reader::ResourceReader;

pub(super) const SCHEMA: &str = "ukmm-native-bnp-cache-v5";
const MAGIC: &[u8; 8] = b"UKBNPC02";

#[derive(Serialize, Deserialize)]
struct Header {
    key: String,
    meta: Meta,
    manifests: HashMap<PathBuf, Manifest>,
    rstb_layers: HashMap<PathBuf, HashMap<smartstring::alias::String, u32>>,
    base_priority: bool,
    dependencies: Vec<(smartstring::alias::String, smartstring::alias::String)>,
}

pub(super) fn archive_digest(path: &Path) -> Result<String> {
    let mut reader = BufReader::with_capacity(1024 * 1024, std::fs::File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Check dump file stamps once for each registered settings context. Updating
/// the game dump, then restarting/reloading settings invalidates decoded BNPs.
pub(super) fn dump_signature(dump: &ResourceReader) -> Result<String> {
    let source = dump.source_ser();
    let value: serde_json::Value = serde_json::from_str(&source)?;
    let mut roots = Vec::<PathBuf>::new();
    for key in ["content_dir", "update_dir", "aoc_dir"] {
        if let Some(path) = value.get(key).and_then(serde_json::Value::as_str) {
            if !path.is_empty() {
                roots.push(path.into());
            }
        }
    }
    if roots.is_empty() {
        roots.push(dump.source().host_path().to_owned());
    }
    let mut digest = Sha256::new();
    digest.update(source.as_bytes());
    for root in roots {
        digest.update(root.to_string_lossy().as_bytes());
        if root.is_file() {
            hash_stamp(&mut digest, &root)?;
        } else if root.is_dir() {
            let mut paths = jwalk::WalkDir::new(&root)
                .into_iter()
                .map(|entry| entry.map(|e| e.path()))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            paths.sort();
            for path in paths {
                digest.update(path.strip_prefix(&root)?.to_string_lossy().as_bytes());
                hash_stamp(&mut digest, &path)?;
            }
        } else {
            digest.update(b"missing");
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn hash_stamp(digest: &mut Sha256, path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    digest.update(metadata.len().to_le_bytes());
    digest.update(
        metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
            .to_le_bytes(),
    );
    Ok(())
}

pub(super) fn cache_key(archive: &str, context: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(SCHEMA);
    digest.update(env!("CARGO_PKG_VERSION"));
    digest.update(archive);
    digest.update(context);
    format!("{:x}", digest.finalize())
}

pub(super) fn save(path: &Path, key: &str, memory: &MemoryMod) -> Result<()> {
    let parent = path.parent().context("BNP cache has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = BufWriter::with_capacity(1024 * 1024, temporary.as_file_mut());
        writer.write_all(MAGIC)?;
        let header = serde_json::to_vec(&Header {
            key: key.into(),
            meta: memory.meta.clone(),
            manifests: memory.manifests.clone(),
            rstb_layers: memory.rstb_layers.clone(),
            base_priority: memory.base_priority,
            dependencies: memory.dependencies.clone(),
        })?;
        writer.write_all(&(header.len() as u64).to_le_bytes())?;
        writer.write_all(&Sha256::digest(&header))?;
        writer.write_all(&header)?;
        writer.write_all(&(memory.resources.len() as u64).to_le_bytes())?;
        let writer = Mutex::new(writer);
        // Independent records avoid holding a second uncompressed copy of the
        // whole mod; compression can run on multiple workers.
        memory
            .resources
            .par_iter()
            .try_for_each(|(name, resource)| -> Result<()> {
                let bytes =
                    minicbor_ser::to_vec(resource.as_ref()).map_err(|e| anyhow::anyhow!("{e}"))?;
                let compressed = zstd::bulk::compress(&bytes, 1)?;
                let name = name
                    .to_str()
                    .context("Cache resource path is not UTF-8")?
                    .as_bytes();
                let mut writer = writer.lock();
                writer.write_all(&(name.len() as u32).to_le_bytes())?;
                writer.write_all(name)?;
                writer.write_all(&(bytes.len() as u64).to_le_bytes())?;
                writer.write_all(&(compressed.len() as u64).to_le_bytes())?;
                let mut checksum = Sha256::new();
                checksum.update(name);
                checksum.update((bytes.len() as u64).to_le_bytes());
                checksum.update(&compressed);
                writer.write_all(&checksum.finalize())?;
                writer.write_all(&compressed)?;
                Ok(())
            })?;
        writer.into_inner().flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(super) fn load(path: &Path, key: &str) -> Result<MemoryMod> {
    let file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    anyhow_ext::ensure!(&magic == MAGIC, "Unsupported native BNP cache version");
    let header_len = read_u64(&mut reader)?;
    anyhow_ext::ensure!(
        header_len <= file_len && header_len <= 64 * 1024 * 1024,
        "Invalid cache header size"
    );
    let mut checksum = [0; 32];
    reader.read_exact(&mut checksum)?;
    let mut header = vec![0; header_len as usize];
    reader.read_exact(&mut header)?;
    anyhow_ext::ensure!(
        Sha256::digest(&header).as_slice() == checksum,
        "Cache header checksum mismatch"
    );
    let header: Header = serde_json::from_slice(&header)?;
    anyhow_ext::ensure!(header.key == key, "BNP cache context mismatch");
    let count = read_u64(&mut reader)?;
    anyhow_ext::ensure!(
        count <= 1_000_000 && count <= file_len / 52,
        "Invalid cache resource count"
    );
    let mut records = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut length = [0; 4];
        reader.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        anyhow_ext::ensure!(length <= 65536, "Invalid cache resource path length");
        let mut name = vec![0; length];
        reader.read_exact(&mut name)?;
        let name = PathBuf::from(String::from_utf8(name)?);
        let raw_len = read_u64(&mut reader)?;
        let compressed_len = read_u64(&mut reader)?;
        anyhow_ext::ensure!(
            raw_len <= 512 * 1024 * 1024 && compressed_len <= file_len,
            "Invalid cache resource size"
        );
        reader.read_exact(&mut checksum)?;
        let mut compressed = vec![0; compressed_len as usize];
        reader.read_exact(&mut compressed)?;
        let mut digest = Sha256::new();
        digest.update(name.to_str().context("Invalid cache path")?.as_bytes());
        digest.update(raw_len.to_le_bytes());
        digest.update(&compressed);
        anyhow_ext::ensure!(
            digest.finalize().as_slice() == checksum,
            "Cache resource checksum mismatch"
        );
        records.push((name, raw_len as usize, compressed));
    }
    anyhow_ext::ensure!(
        reader.read(&mut [0; 1])? == 0,
        "Trailing data in native BNP cache"
    );
    let resources = records
        .into_par_iter()
        .map(|(name, raw_len, compressed)| -> Result<_> {
            let bytes = zstd::bulk::decompress(&compressed, raw_len)?;
            anyhow_ext::ensure!(bytes.len() == raw_len, "Cache resource length mismatch");
            let resource: ResourceData = minicbor_ser::from_slice(&bytes)?;
            Ok((name, Arc::new(resource)))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    anyhow_ext::ensure!(
        resources.len() == count as usize,
        "Duplicate cache resources"
    );
    Ok(MemoryMod {
        meta: header.meta,
        resources,
        manifests: header.manifests,
        rstb_layers: header.rstb_layers,
        base_priority: header.base_priority,
        dependencies: header.dependencies,
    })
}

fn read_u64(reader: &mut impl Read) -> Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uk_content::{map::unit::MapUnit, prelude::Endian, resource::MergeableResource};
    use uk_mod::{ModCategory, ModPlatform};

    fn fixture() -> MemoryMod {
        MemoryMod {
            meta: Meta {
                api: "0.17.1".into(),
                name: "Cache fixture".into(),
                version: "1.0".into(),
                author: "Tester".into(),
                category: ModCategory::Other,
                description: "".into(),
                platform: ModPlatform::Specific(Endian::Big),
                url: None,
                options: vec![],
                masters: Default::default(),
            },
            resources: [
                (
                    "Audit/bytes.bin".into(),
                    Arc::new(ResourceData::Binary(vec![1, 2, 3, 4])),
                ),
                (
                    "Map/fixture.mubin".into(),
                    Arc::new(ResourceData::Mergeable(MergeableResource::MapUnit(
                        Box::new(MapUnit {
                            pos_x: Some(-3224.42627),
                            ..Default::default()
                        }),
                    ))),
                ),
                (
                    "Actor/AIProgram/fixture.baiprog".into(),
                    Arc::new(ResourceData::Mergeable(MergeableResource::AIProgram(Box::new(
                        uk_content::actor::params::aiprog::AIProgram {
                            extra: roead::aamp::ParameterList::new().with_object("Custom",
                                roead::aamp::ParameterObject::new().with_parameter("Padding",
                                    roead::aamp::Parameter::StringRef(" ".repeat(1024).into()))),
                            ..Default::default()
                        },
                    )))),
                ),
            ]
            .into_iter()
            .collect(),
            manifests: [(
                PathBuf::new(),
                Manifest {
                    content_files: ["Audit/bytes.bin".into()].into_iter().collect(),
                    aoc_files: Default::default(),
                },
            )]
            .into_iter()
            .collect(),
            rstb_layers: [(
                PathBuf::new(),
                [("Audit/bytes.bin".into(), 100)].into_iter().collect(),
            )]
            .into_iter()
            .collect(),
            base_priority: true,
            dependencies: vec![("Foundation".into(), "1.0".into())],
        }
    }

    #[test]
    fn decoded_cache_round_trip_preserves_typed_resources_and_replaces_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.cache");
        let original = fixture();
        save(&path, "context", &original).unwrap();
        save(&path, "context", &original).unwrap();
        let restored = load(&path, "context").unwrap();
        assert_eq!(restored.meta, original.meta);
        assert_eq!(restored.manifests, original.manifests);
        assert_eq!(restored.resources, original.resources);
        assert_eq!(restored.rstb_layers, original.rstb_layers);
        assert_eq!(restored.dependencies, original.dependencies);
        assert!(restored.base_priority);
        assert!(load(&path, "other-context").is_err());
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(load(&path, "context").is_err());
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(load(&path, "context").is_err());
    }

    #[test]
    fn cache_identity_reuses_copies_and_invalidates_archive_and_dump_changes() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("one.bnp");
        let copy = temp.path().join("two.bnp");
        std::fs::write(&archive, b"archive bytes").unwrap();
        std::fs::copy(&archive, &copy).unwrap();
        assert_eq!(
            archive_digest(&archive).unwrap(),
            archive_digest(&copy).unwrap()
        );
        std::fs::write(&copy, b"archive edits").unwrap();
        assert_ne!(
            archive_digest(&archive).unwrap(),
            archive_digest(&copy).unwrap()
        );
        let root = temp.path().join("dump");
        std::fs::create_dir_all(root.join("content")).unwrap();
        let dump = ResourceReader::from_unpacked_mod(&root).unwrap();
        let before = dump_signature(&dump).unwrap();
        std::fs::write(root.join("content/new.bin"), b"new resource").unwrap();
        let after = dump_signature(&dump).unwrap();
        assert_ne!(before, after);
        assert_ne!(cache_key("archive", "JPja"), cache_key("archive", "USen"));
    }
}
