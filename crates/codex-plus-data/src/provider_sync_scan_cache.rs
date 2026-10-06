//! Startup-only scan facts. No messages, instructions or session_meta text are persisted here.
//! Manual repair always scans the original rollouts. Writes still verify their full SHA-256.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 1;
const MAX_CACHE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_META_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Fingerprint {
    size: u64,
    modified_ns: u64,
    created_ns: Option<u64>,
    change_time: u64,
}

fn timestamp_ns(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .try_into()
        .ok()
}

impl Fingerprint {
    pub(crate) fn from_file(file: &File) -> Option<Self> {
        let metadata = file.metadata().ok()?;
        #[cfg(windows)]
        let change_time = codex_plus_core::file_change_time(file).ok()?;
        #[cfg(unix)]
        let change_time = {
            use std::os::unix::fs::MetadataExt;
            (metadata.ctime() as u64)
                .checked_mul(1_000_000_000)?
                .checked_add(metadata.ctime_nsec() as u64)?
        };
        #[cfg(not(any(windows, unix)))]
        let change_time = timestamp_ns(metadata.modified().ok()?)?;
        Some(Self {
            size: metadata.len(),
            modified_ns: timestamp_ns(metadata.modified().ok()?)?,
            created_ns: metadata.created().ok().and_then(timestamp_ns),
            change_time,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MetaLocation {
    pub offset: u64,
    pub length: u64,
    pub sha256: String,
}

impl MetaLocation {
    pub(crate) fn new(offset: u64, line: &str) -> Self {
        Self {
            offset,
            length: line.len() as u64,
            sha256: digest(line.as_bytes()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ScanFacts {
    pub sha256: String,
    pub thread_id: Option<String>,
    pub cwd: Option<String>,
    pub providers: Vec<String>,
    pub meta_locations: Vec<MetaLocation>,
    pub non_root_agent: bool,
    pub has_user_event: bool,
    pub has_encrypted_content: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    fingerprint: Fingerprint,
    facts: ScanFacts,
    checksum: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ScanCache {
    version: u32,
    entries: HashMap<PathBuf, Entry>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl ScanCache {
    fn path(home: &Path) -> PathBuf {
        home.join("tmp/provider-sync-scan-cache-v1.json")
    }

    pub(crate) fn load(home: &Path) -> Self {
        let path = Self::path(home);
        let cached = (|| {
            if fs::metadata(&path).ok()?.len() > MAX_CACHE_BYTES {
                return None;
            }
            let cache: Self = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
            (cache.version == VERSION).then_some(cache)
        })();
        cached.unwrap_or(Self {
            version: VERSION,
            entries: HashMap::new(),
        })
    }

    pub(crate) fn retain_paths(&mut self, paths: &[PathBuf]) {
        let existing = paths.iter().collect::<std::collections::HashSet<_>>();
        self.entries.retain(|path, _| existing.contains(path));
    }

    pub(crate) fn get(&self, path: &Path, file: &mut File) -> Option<(ScanFacts, Vec<String>)> {
        let entry = self.entries.get(path)?;
        let fingerprint = Fingerprint::from_file(file)?;
        if fingerprint != entry.fingerprint
            || digest(&serde_json::to_vec(&entry.facts).ok()?) != entry.checksum
            || entry.facts.meta_locations.len() != entry.facts.providers.len()
        {
            return None;
        }
        let mut lines = Vec::new();
        let mut previous_end = 0;
        for location in &entry.facts.meta_locations {
            let end = location.offset.checked_add(location.length)?;
            if location.offset < previous_end
                || end > fingerprint.size
                || location.length > MAX_META_BYTES
            {
                return None;
            }
            file.seek(SeekFrom::Start(location.offset)).ok()?;
            let mut bytes = vec![0; location.length.try_into().ok()?];
            file.read_exact(&mut bytes).ok()?;
            if digest(&bytes) != location.sha256 {
                return None;
            }
            lines.push(String::from_utf8(bytes).ok()?);
            previous_end = end;
        }
        if Fingerprint::from_file(file)? != fingerprint {
            return None;
        }
        Some((entry.facts.clone(), lines))
    }

    pub(crate) fn insert(&mut self, path: &Path, fingerprint: Fingerprint, facts: ScanFacts) {
        let Ok(bytes) = serde_json::to_vec(&facts) else {
            return;
        };
        self.entries.insert(
            path.to_owned(),
            Entry {
                fingerprint,
                facts,
                checksum: digest(&bytes),
            },
        );
    }

    pub(crate) fn refresh_rewritten(
        &mut self,
        path: &Path,
        provider: &str,
        originals: &[String],
        sha256: &str,
    ) {
        let refreshed = (|| -> Option<(Fingerprint, ScanFacts)> {
            let entry = self.entries.get(path)?;
            let mut facts = entry.facts.clone();
            if facts.meta_locations.len() != originals.len() {
                return None;
            }
            let mut displacement: i64 = 0;
            for (location, original) in facts.meta_locations.iter_mut().zip(originals) {
                let mut record: serde_json::Value = serde_json::from_str(original).ok()?;
                let payload = record.get_mut("payload")?.as_object_mut()?;
                let rewritten = if payload
                    .get("model_provider")
                    .and_then(serde_json::Value::as_str)
                    == Some(provider)
                {
                    original.clone()
                } else {
                    payload.insert("model_provider".into(), provider.into());
                    serde_json::to_string(&record).ok()?
                };
                let offset = location.offset.checked_add_signed(displacement)?;
                displacement =
                    displacement.checked_add(rewritten.len() as i64 - original.len() as i64)?;
                *location = MetaLocation::new(offset, &rewritten);
            }
            facts.sha256 = sha256.to_owned();
            facts.providers.fill(provider.to_owned());
            let mut file = File::open(path).ok()?;
            let fingerprint = Fingerprint::from_file(&file)?;
            // The body may change after replacement but before this refresh. Bind the
            // cached facts to the complete output, not just its metadata lines.
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer).ok()?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
            if format!("{:x}", hasher.finalize()) != sha256
                || Fingerprint::from_file(&file)? != fingerprint
            {
                return None;
            }
            let probe = Entry {
                fingerprint: fingerprint.clone(),
                checksum: digest(&serde_json::to_vec(&facts).ok()?),
                facts: facts.clone(),
            };
            let probe_cache = Self {
                version: VERSION,
                entries: HashMap::from([(path.to_owned(), probe)]),
            };
            probe_cache.get(path, &mut file)?;
            Some((fingerprint, facts))
        })();
        if let Some((fingerprint, facts)) = refreshed {
            self.insert(path, fingerprint, facts);
        } else {
            self.entries.remove(path);
        }
    }

    pub(crate) fn save(&self, home: &Path) {
        let saved = (|| -> anyhow::Result<()> {
            let path = Self::path(home);
            fs::create_dir_all(path.parent().unwrap())?;
            let bytes = serde_json::to_vec(self)?;
            if bytes.len() as u64 > MAX_CACHE_BYTES {
                anyhow::bail!("scan cache limit exceeded");
            }
            codex_plus_core::settings::atomic_write(&path, &bytes)
        })()
        .is_ok();
        let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
            "provider_sync.scan_cache_saved",
            serde_json::json!({"saved": saved, "files": self.entries.len()}),
        );
    }
}
