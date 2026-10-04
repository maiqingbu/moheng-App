use crate::repositories::export::CurrentFinalChapter;
use libsql_rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const EXPORT_CHUNK_CHARACTERS: usize = crate::repositories::export::MAX_BODY_CHUNK_CHARACTERS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportCapability {
    pub available: bool,
    pub reason: Option<String>,
    pub supported_formats: Vec<String>,
    pub current_final_chapter_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub status: String,
    pub request_id: String,
    pub completed_chapters: u64,
    pub total_chapters: u64,
    pub completed_characters: u64,
    pub total_characters: u64,
    pub current_chapter_number: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportChapterRef {
    pub chapter_id: String,
    pub chapter_number: u64,
    pub title: String,
    pub artifact_id: String,
    pub version: u64,
    pub payload_hash: String,
    pub character_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportReceipt {
    pub schema: String,
    pub status: String,
    pub receipt_id: String,
    pub request_id: String,
    pub project_id: String,
    pub format: String,
    pub file_name: String,
    pub final_path: String,
    pub created_at_epoch_ms: u128,
    pub elapsed_ms: u128,
    pub source_project_revision: u64,
    pub chapters: Vec<ExportChapterRef>,
    pub source_character_count: u64,
    pub output_character_count: u64,
    pub output_byte_count: u64,
    pub sha256: String,
    pub max_chunk_characters: usize,
    pub max_working_buffer_bytes: u64,
}

struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
    bytes_written: u64,
}

struct ExportJob<'a> {
    project_id: &'a str,
    request_id: &'a str,
    key: &'a str,
}

impl<W> HashingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes_written: 0,
        }
    }

    fn digest(&self) -> String {
        hex::encode(self.hasher.clone().finalize())
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..written]);
        self.bytes_written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub fn query_capability(
    connection: &Connection,
    project_id: &str,
) -> Result<ExportCapability, String> {
    let count = crate::repositories::export::current_final_chapter_count(connection, project_id)?;
    let available = count > 0;
    Ok(ExportCapability {
        available,
        reason: (!available).then(|| "NO_CURRENT_FINAL_CHAPTERS".to_string()),
        supported_formats: if available {
            vec!["TXT".into()]
        } else {
            Vec::new()
        },
        current_final_chapter_count: count,
    })
}

const SCAN_BUFFER_BYTES: usize = 64 * 1024;
const SCAN_OVERLAP_BYTES: usize = 128;
const EXPORT_SOURCE_WORKING_BUFFER_BYTES: usize =
    crate::repositories::export::JSON_READ_BUFFER_BYTES + EXPORT_CHUNK_CHARACTERS * 4;
const SCAN_WORKING_BUFFER_BYTES: usize = (SCAN_BUFFER_BYTES + SCAN_OVERLAP_BYTES) * 2;

pub fn create_export(
    connection: &Connection,
    project_dir: &Path,
    project_id: &str,
    request_id: &str,
    format: &str,
) -> Result<ExportReceipt, String> {
    if request_id.trim().is_empty() || request_id.chars().count() > 200 {
        return Err("EXPORT_REQUEST_ID_INVALID".into());
    }
    if format != "TXT" {
        return Err("EXPORT_FORMAT_UNSUPPORTED".into());
    }

    let key = export_key(project_id, request_id, format);
    let exports_dir = ensure_exports_dir(project_dir)?;
    let final_dir = exports_dir.join(format!("export-{key}"));
    if let Some(receipt) = read_existing_receipt(&final_dir)? {
        return Ok(receipt);
    }

    let staging_dir = exports_dir.join(format!(".staging-{key}"));
    match fs::create_dir(&staging_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if let Some(receipt) = read_existing_receipt(&final_dir)? {
                return Ok(receipt);
            }
            return Err("EXPORT_ALREADY_RUNNING".into());
        }
        Err(_) => return Err("EXPORT_IO_ERROR".into()),
    }
    let progress_path = exports_dir.join(format!(".progress-{key}.json"));
    let job = ExportJob {
        project_id,
        request_id,
        key: &key,
    };

    let receipt = run_staged_write(&staging_dir, &progress_path, || {
        crate::repositories::export::with_current_final_chapters(
            connection,
            project_id,
            |revision, chapters| {
                create_staged_export_from_snapshot(
                    connection,
                    &staging_dir,
                    &progress_path,
                    &job,
                    revision,
                    chapters,
                )
            },
        )
    })?;

    if fs::rename(&staging_dir, &final_dir).is_err() {
        let _ = fs::remove_dir_all(&staging_dir);
        let _ = fs::remove_file(&progress_path);
        if let Some(receipt) = read_existing_receipt(&final_dir)? {
            return Ok(receipt);
        }
        return Err("EXPORT_FINALIZE_FAILED".into());
    }
    let _ = fs::remove_file(&progress_path);
    Ok(receipt)
}

fn create_staged_export_from_snapshot(
    connection: &Connection,
    staging_dir: &Path,
    progress_path: &Path,
    job: &ExportJob<'_>,
    revision: u64,
    mut chapters: Vec<CurrentFinalChapter>,
) -> Result<ExportReceipt, String> {
    let started = Instant::now();
    if chapters.is_empty() {
        return Err("NO_CURRENT_FINAL_CHAPTERS".into());
    }
    for chapter in &mut chapters {
        chapter.character_count = crate::repositories::export::stream_final_chapter_body(
            connection,
            job.project_id,
            chapter,
            EXPORT_CHUNK_CHARACTERS,
            |_| Ok(()),
        )?;
    }
    let total_characters = chapters.iter().map(|chapter| chapter.character_count).sum();
    publish_progress(
        progress_path,
        &ExportProgress {
            status: "RUNNING".into(),
            request_id: job.request_id.into(),
            completed_chapters: 0,
            total_chapters: chapters.len() as u64,
            completed_characters: 0,
            total_characters,
            current_chapter_number: chapters.first().map(|chapter| chapter.chapter_number),
        },
    )?;

    let output_path = staging_dir.join("book.txt");
    let file = File::create(&output_path).map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    let mut writer = HashingWriter::new(file);
    let mut completed_characters = 0_u64;
    let mut output_characters = 0_u64;
    let mut max_chunk_characters = 0_usize;
    let max_working_buffer_bytes = EXPORT_SOURCE_WORKING_BUFFER_BYTES as u64;
    for (index, chapter) in chapters.iter().enumerate() {
        let heading = format!("第{}章 {}\n\n", chapter.chapter_number, chapter.title);
        writer
            .write_all(heading.as_bytes())
            .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
        output_characters += heading.chars().count() as u64;

        let mut streamed_characters = 0_u64;
        let streamed = crate::repositories::export::stream_final_chapter_body(
            connection,
            job.project_id,
            chapter,
            EXPORT_CHUNK_CHARACTERS,
            |chunk| {
                let chunk_characters = chunk.chars().count();
                if chunk_characters > EXPORT_CHUNK_CHARACTERS {
                    return Err("EXPORT_CHUNK_BOUND_EXCEEDED".into());
                }
                writer
                    .write_all(chunk.as_bytes())
                    .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
                completed_characters += chunk_characters as u64;
                output_characters += chunk_characters as u64;
                max_chunk_characters = max_chunk_characters.max(chunk_characters);
                streamed_characters += chunk_characters as u64;
                publish_progress(
                    progress_path,
                    &ExportProgress {
                        status: "RUNNING".into(),
                        request_id: job.request_id.into(),
                        completed_chapters: index as u64,
                        total_chapters: chapters.len() as u64,
                        completed_characters,
                        total_characters,
                        current_chapter_number: Some(chapter.chapter_number),
                    },
                )
            },
        )?;
        if streamed != chapter.character_count || streamed_characters != chapter.character_count {
            return Err("EXPORT_SOURCE_CHANGED".into());
        }
        writer
            .write_all(b"\n\n")
            .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
        output_characters += 2;
        completed_characters = chapters[..=index]
            .iter()
            .map(|chapter| chapter.character_count)
            .sum();
        publish_progress(
            progress_path,
            &ExportProgress {
                status: "RUNNING".into(),
                request_id: job.request_id.into(),
                completed_chapters: index as u64 + 1,
                total_chapters: chapters.len() as u64,
                completed_characters,
                total_characters,
                current_chapter_number: chapters.get(index + 1).map(|next| next.chapter_number),
            },
        )?;
    }
    writer.flush().map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    writer
        .inner
        .sync_all()
        .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    scan_export_file(&output_path)?;

    let chapter_refs = chapters
        .iter()
        .map(|chapter| ExportChapterRef {
            chapter_id: chapter.chapter_id.clone(),
            chapter_number: chapter.chapter_number,
            title: chapter.title.clone(),
            artifact_id: chapter.artifact_id.clone(),
            version: chapter.version,
            payload_hash: chapter.payload_hash.clone(),
            character_count: chapter.character_count,
        })
        .collect();
    let final_path = staging_dir
        .parent()
        .unwrap_or(staging_dir)
        .join(format!("export-{}", job.key))
        .join("book.txt");
    let receipt = ExportReceipt {
        schema: "moheng.export-receipt.v1".into(),
        status: "COMPLETED".into(),
        receipt_id: format!("export-receipt:{}", job.key),
        request_id: job.request_id.into(),
        project_id: job.project_id.into(),
        format: "TXT".into(),
        file_name: "book.txt".into(),
        final_path: final_path.to_string_lossy().into_owned(),
        created_at_epoch_ms: now_epoch_ms(),
        elapsed_ms: started.elapsed().as_millis(),
        source_project_revision: revision,
        chapters: chapter_refs,
        source_character_count: total_characters,
        output_character_count: output_characters,
        output_byte_count: writer.bytes_written,
        sha256: writer.digest(),
        max_chunk_characters,
        max_working_buffer_bytes: max_working_buffer_bytes.max(SCAN_WORKING_BUFFER_BYTES as u64),
    };
    let receipt_path = staging_dir.join("receipt.json");
    let receipt_bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|_| "EXPORT_RECEIPT_SERIALIZE_FAILED".to_string())?;
    let mut receipt_file = File::create(receipt_path).map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    receipt_file
        .write_all(&receipt_bytes)
        .and_then(|()| receipt_file.sync_all())
        .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    publish_progress(
        progress_path,
        &ExportProgress {
            status: "FINALIZING".into(),
            request_id: job.request_id.into(),
            completed_chapters: chapters.len() as u64,
            total_chapters: chapters.len() as u64,
            completed_characters: total_characters,
            total_characters,
            current_chapter_number: None,
        },
    )?;
    Ok(receipt)
}

fn run_staged_write<T>(
    staging_dir: &Path,
    progress_path: &Path,
    write: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match write() {
        Ok(result) => Ok(result),
        Err(error) => {
            let _ = fs::remove_dir_all(staging_dir);
            let _ = fs::remove_file(progress_path);
            Err(error)
        }
    }
}

fn publish_progress(path: &Path, progress: &ExportProgress) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "EXPORT_IO_ERROR".to_string())?;
    let temporary_path = path.with_file_name(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
    let bytes =
        serde_json::to_vec(progress).map_err(|_| "EXPORT_PROGRESS_SERIALIZE_FAILED".to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        let _ = fs::remove_file(&temporary_path);
        return Err("EXPORT_IO_ERROR".into());
    }
    if fs::rename(&temporary_path, path).is_err() {
        let _ = fs::remove_file(&temporary_path);
        return Err("EXPORT_IO_ERROR".into());
    }
    Ok(())
}

fn scan_export_file(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    let mut buffer = [0_u8; SCAN_BUFFER_BYTES];
    let mut overlap = Vec::new();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
        if read == 0 {
            break;
        }
        let mut window = std::mem::take(&mut overlap);
        window.extend_from_slice(&buffer[..read]);
        if contains_private_canary(&window) || contains_credential_pattern(&window) {
            return Err("EXPORT_SECURITY_SCAN_FAILED".into());
        }
        let keep_from = window.len().saturating_sub(SCAN_OVERLAP_BYTES);
        overlap.extend_from_slice(&window[keep_from..]);
    }
    Ok(())
}

fn contains_private_canary(bytes: &[u8]) -> bool {
    [
        b"PRIVATE_METADATA_CANARY".as_slice(),
        b"INTERNAL_PROMPT_CANARY".as_slice(),
        b"HARNESS_PRIVATE_CANARY".as_slice(),
        b"CANDIDATE_BODY_CANARY".as_slice(),
        b"CANDIDATE_PRIVATE_CANARY".as_slice(),
        b"project.db".as_slice(),
        b"stack backtrace".as_slice(),
        b"BEGIN PRIVATE KEY".as_slice(),
    ]
    .iter()
    .any(|pattern| bytes.windows(pattern.len()).any(|part| part == *pattern))
}

fn contains_credential_pattern(bytes: &[u8]) -> bool {
    let lower = bytes.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
    [
        b"sk-".as_slice(),
        b"ghp_".as_slice(),
        b"github_pat_".as_slice(),
        b"xoxb-".as_slice(),
        b"aiza".as_slice(),
        b"bearer ".as_slice(),
    ]
    .iter()
    .any(|prefix| {
        lower
            .windows(prefix.len())
            .enumerate()
            .filter(|(_, part)| *part == *prefix)
            .any(|(index, _)| {
                lower[index + prefix.len()..]
                    .iter()
                    .take_while(|byte| {
                        byte.is_ascii_alphanumeric() || **byte == b'_' || **byte == b'-'
                    })
                    .count()
                    >= 16
            })
    })
}

pub fn query_receipt(
    project_dir: &Path,
    project_id: &str,
    request_id: &str,
) -> Result<Value, String> {
    if request_id.trim().is_empty() || request_id.chars().count() > 200 {
        return Err("EXPORT_REQUEST_ID_INVALID".into());
    }
    let key = export_key(project_id, request_id, "TXT");
    let exports_dir =
        existing_exports_dir(project_dir)?.ok_or_else(|| "EXPORT_RECEIPT_NOT_FOUND".to_string())?;
    let final_dir = exports_dir.join(format!("export-{key}"));
    if let Some(receipt) = read_existing_receipt(&final_dir)? {
        return serde_json::to_value(receipt).map_err(|_| "EXPORT_RECEIPT_SERIALIZE_FAILED".into());
    }
    let progress_path = exports_dir.join(format!(".progress-{key}.json"));
    let progress_meta =
        fs::symlink_metadata(&progress_path).map_err(|_| "EXPORT_RECEIPT_NOT_FOUND".to_string())?;
    if progress_meta.file_type().is_symlink() || !progress_meta.is_file() {
        return Err("EXPORT_PROGRESS_CORRUPT".into());
    }
    let bytes = fs::read(progress_path).map_err(|_| "EXPORT_RECEIPT_NOT_FOUND".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "EXPORT_PROGRESS_CORRUPT".into())
}

pub fn query_latest_receipt(project_dir: &Path) -> Result<Value, String> {
    let Some(exports_dir) = existing_exports_dir(project_dir)? else {
        return Ok(json!({"status":"NONE"}));
    };
    let entries = fs::read_dir(exports_dir).map_err(|_| "EXPORT_RECEIPT_QUERY_FAILED")?;
    let mut latest: Option<ExportReceipt> = None;
    for entry in entries {
        let entry = entry.map_err(|_| "EXPORT_RECEIPT_QUERY_FAILED".to_string())?;
        let file_type = entry
            .file_type()
            .map_err(|_| "EXPORT_RECEIPT_QUERY_FAILED".to_string())?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("export-") {
            continue;
        }
        let receipt_path = entry.path().join("receipt.json");
        let receipt = match read_receipt(&receipt_path) {
            Ok(receipt) if receipt.status == "COMPLETED" => receipt,
            _ => continue,
        };
        if latest.as_ref().is_none_or(|current| {
            receipt.created_at_epoch_ms > current.created_at_epoch_ms
                || (receipt.created_at_epoch_ms == current.created_at_epoch_ms
                    && receipt.receipt_id > current.receipt_id)
        }) {
            latest = Some(receipt);
        }
    }
    match latest {
        Some(receipt) => {
            serde_json::to_value(receipt).map_err(|_| "EXPORT_RECEIPT_SERIALIZE_FAILED".into())
        }
        None => Ok(json!({"status":"NONE"})),
    }
}

fn read_receipt(path: &Path) -> Result<ExportReceipt, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "EXPORT_RECEIPT_READ_FAILED")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("EXPORT_RECEIPT_CORRUPT".into());
    }
    let bytes = fs::read(path).map_err(|_| "EXPORT_RECEIPT_READ_FAILED".to_string())?;
    let receipt: ExportReceipt =
        serde_json::from_slice(&bytes).map_err(|_| "EXPORT_RECEIPT_CORRUPT".to_string())?;
    if receipt.status != "COMPLETED" {
        return Err("EXPORT_RECEIPT_CORRUPT".into());
    }
    Ok(receipt)
}

fn ensure_exports_dir(project_dir: &Path) -> Result<PathBuf, String> {
    let path = project_dir.join("exports");
    fs::create_dir_all(&path).map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| "EXPORT_IO_ERROR".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("EXPORT_PATH_INVALID".into());
    }
    Ok(path)
}

fn existing_exports_dir(project_dir: &Path) -> Result<Option<PathBuf>, String> {
    let path = project_dir.join("exports");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("EXPORT_RECEIPT_QUERY_FAILED".into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("EXPORT_PATH_INVALID".into());
    }
    Ok(Some(path))
}

fn read_existing_receipt(path: &Path) -> Result<Option<ExportReceipt>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("EXPORT_RECEIPT_READ_FAILED".into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("EXPORT_PATH_INVALID".into());
    }
    read_receipt(&path.join("receipt.json")).map(Some)
}

fn export_key(project_id: &str, request_id: &str, format: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [project_id, request_id, format] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn now_epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingWriter {
        remaining: usize,
    }

    impl Write for FailingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::other("injected export I/O failure"));
            }
            let written = buffer.len().min(self.remaining);
            self.remaining -= written;
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn hashing_writer_propagates_midstream_io_failure() {
        let mut writer = HashingWriter::new(FailingWriter { remaining: 12 });
        assert!(writer.write_all(b"a longer export body").is_err());
        assert_eq!(writer.bytes_written, 12);
    }

    #[test]
    fn failed_staged_write_leaves_no_final_file_or_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let exports_dir = temp.path().join("exports");
        fs::create_dir_all(&exports_dir).unwrap();
        let staging = exports_dir.join(".staging-fault");
        let final_dir = exports_dir.join("export-fault");
        let progress = exports_dir.join(".progress-fault.json");
        fs::create_dir(&staging).unwrap();
        fs::write(&progress, b"{\"status\":\"RUNNING\"}").unwrap();

        let result = run_staged_write(&staging, &progress, || {
            let mut file = File::create(staging.join("book.txt"))
                .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
            file.write_all(b"partial output")
                .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
            let mut failing_writer = HashingWriter::new(FailingWriter { remaining: 8 });
            failing_writer
                .write_all(b"partially written export")
                .map_err(|_| "EXPORT_IO_ERROR".to_string())?;
            Ok(())
        });

        assert_eq!(result.unwrap_err(), "EXPORT_IO_ERROR");
        assert!(!staging.exists());
        assert!(!final_dir.exists());
        assert!(!progress.exists());
    }

    #[test]
    fn query_returns_live_progress_before_receipt_is_finalized() {
        let temp = tempfile::tempdir().unwrap();
        let project_dir = temp.path();
        let exports_dir = project_dir.join("exports");
        fs::create_dir_all(&exports_dir).unwrap();
        let request_id = "progress-request";
        let progress_path = exports_dir.join(format!(
            ".progress-{}.json",
            export_key("project:progress", request_id, "TXT")
        ));
        publish_progress(
            &progress_path,
            &ExportProgress {
                status: "RUNNING".into(),
                request_id: request_id.into(),
                completed_chapters: 1,
                total_chapters: 3,
                completed_characters: 5_000,
                total_characters: 15_000,
                current_chapter_number: Some(2),
            },
        )
        .unwrap();

        let progress = query_receipt(project_dir, "project:progress", request_id).unwrap();
        assert_eq!(progress["status"], "RUNNING");
        assert_eq!(progress["completedChapters"], 1);
        assert_eq!(progress["currentChapterNumber"], 2);
    }

    #[test]
    fn request_id_hash_cannot_escape_the_exports_directory() {
        let key = export_key("project", "../../private", "TXT");
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
