use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use fs2::FileExt;
use serde_json::{Map, Value};

const MAX_HISTORY_RECORDS: usize = 1_000;

#[derive(Clone)]
struct SourceRecord {
    id: String,
    operation: PasteOperation,
    source_host: String,
    source_user: String,
    items: Vec<SourceItem>,
}

#[derive(Clone, Copy)]
enum PasteOperation {
    Copy,
    Move,
    Cut,
    Delete,
}

#[derive(Clone)]
struct SourceItem {
    path: PathBuf,
    display_path: String,
    path_encoding: Option<String>,
    path_data: Option<String>,
    file_type: String,
}

struct PasteOutcome {
    source: SourceItem,
    destination: PathBuf,
    status: ItemStatus,
}

#[derive(PartialEq, Eq)]
enum ItemStatus {
    Pasted,
    Failed(String),
}

struct EncodedPath {
    encoding: &'static str,
    data: String,
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(message) => {
            eprintln!("{message}");
            process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    if env::args_os().len() > 1 {
        return Err("fpaste: does not accept arguments".to_string());
    }

    let history = history_path()?;
    if !history.exists() {
        return Err(format!("fpaste: history not found: {}", history.display()));
    }

    let contents = fs::read_to_string(&history)
        .map_err(|err| format!("fpaste: cannot open history {}: {err}", history.display()))?;
    let record = latest_supported_record(&contents)?;
    let destination_dir = env::current_dir()
        .map_err(|err| format!("fpaste: cannot determine current directory: {err}"))?;

    preflight_destinations(&record, &destination_dir)?;

    let mut outcomes = Vec::with_capacity(record.items.len());
    for item in &record.items {
        let destination = destination_dir.join(final_component(&item.path)?);
        let status = if fs::symlink_metadata(&item.path).is_err() {
            eprintln!("fpaste: not found: {}", item.path.display());
            ItemStatus::Failed("not found".to_string())
        } else {
            match paste_item(record.operation, &item.path, &destination) {
                Ok(()) => {
                    let action = if record.operation.is_copy() {
                        "pasted"
                    } else {
                        "moved"
                    };
                    println!(
                        "{action}: {} -> ./{}",
                        item.path.display(),
                        destination
                            .file_name()
                            .unwrap_or_else(|| OsStr::new(""))
                            .to_string_lossy()
                    );
                    ItemStatus::Pasted
                }
                Err(err) => {
                    let verb = if record.operation.is_copy() {
                        "copy"
                    } else {
                        "move"
                    };
                    eprintln!(
                        "fpaste: cannot {verb} {} to {}: {err}",
                        item.path.display(),
                        destination.display()
                    );
                    ItemStatus::Failed(err.to_string())
                }
            }
        };

        outcomes.push(PasteOutcome {
            source: item.clone(),
            destination,
            status,
        });
    }

    let succeeded = outcomes
        .iter()
        .filter(|outcome| outcome.status == ItemStatus::Pasted)
        .count();
    let failed = outcomes.len() - succeeded;
    println!("fpaste: {succeeded} succeeded, {failed} failed");

    append_paste_history(&history, &record, &destination_dir, &outcomes)?;

    if failed == 0 {
        Ok(())
    } else {
        Err("fpaste: one or more items failed".to_string())
    }
}

impl PasteOperation {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "copy" => Some(Self::Copy),
            "move" => Some(Self::Move),
            "cut" => Some(Self::Cut),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Cut => "cut",
            Self::Delete => "delete",
        }
    }

    fn is_copy(self) -> bool {
        matches!(self, Self::Copy)
    }
}

fn latest_supported_record(contents: &str) -> Result<SourceRecord, String> {
    let mut malformed = None;

    for line in contents.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_source_record(line) {
            Ok(Some(record)) => return Ok(record),
            Ok(None) => {}
            Err(err) => {
                malformed.get_or_insert(err);
            }
        }
    }

    if let Some(err) = malformed {
        Err(format!("fpaste: malformed history record: {err}"))
    } else {
        Err("fpaste: no supported history records".to_string())
    }
}

fn parse_source_record(line: &str) -> Result<Option<SourceRecord>, String> {
    let value: Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "record is not an object".to_string())?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .and_then(PasteOperation::from_str);
    let Some(operation) = operation else {
        return Ok(None);
    };

    let items_value = object
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "supported record has no items array".to_string())?;
    let mut items = Vec::with_capacity(items_value.len());
    for item in items_value {
        let item_object = item
            .as_object()
            .ok_or_else(|| "item is not an object".to_string())?;
        items.push(parse_source_item(item_object)?);
    }

    Ok(Some(SourceRecord {
        id: string_field(object, "id").unwrap_or_default(),
        operation,
        source_host: string_field(object, "source_host").unwrap_or_else(|| "unknown".to_string()),
        source_user: string_field(object, "source_user").unwrap_or_else(|| "unknown".to_string()),
        items,
    }))
}

fn parse_source_item(object: &Map<String, Value>) -> Result<SourceItem, String> {
    let path_encoding = string_field(object, "path_encoding");
    let path_data = string_field(object, "path_data");
    let fallback = string_field(object, "path");
    let path = reconstruct_path(
        path_encoding.as_deref(),
        path_data.as_deref(),
        fallback.as_deref(),
    )
    .map_err(|err| format!("cannot reconstruct path for item: {err}"))?;
    let display_path = fallback.unwrap_or_else(|| path.display().to_string());
    let file_type = string_field(object, "file_type").unwrap_or_else(|| "other".to_string());

    Ok(SourceItem {
        path,
        display_path,
        path_encoding,
        path_data,
        file_type,
    })
}

fn string_field(object: &Map<String, Value>, name: &str) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn reconstruct_path(
    encoding: Option<&str>,
    data: Option<&str>,
    fallback: Option<&str>,
) -> Result<PathBuf, String> {
    if let (Some(encoding), Some(data)) = (encoding, data) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|err| err.to_string())?;
        if let Some(path) = path_from_native_bytes(encoding, &bytes)? {
            return Ok(path);
        }
    }

    fallback
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "missing usable path fields".to_string())
}

#[cfg(unix)]
fn path_from_native_bytes(encoding: &str, bytes: &[u8]) -> Result<Option<PathBuf>, String> {
    use std::os::unix::ffi::OsStringExt;

    if encoding == "unix-bytes-base64" {
        return Ok(Some(PathBuf::from(OsString::from_vec(bytes.to_vec()))));
    }
    if encoding == "utf8-base64" {
        return String::from_utf8(bytes.to_vec())
            .map(|value| Some(PathBuf::from(value)))
            .map_err(|err| err.to_string());
    }
    Ok(None)
}

#[cfg(windows)]
fn path_from_native_bytes(encoding: &str, bytes: &[u8]) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;

    if encoding == "windows-utf16le-base64" {
        if bytes.len() % 2 != 0 {
            return Err("odd number of UTF-16 bytes".to_string());
        }
        let wide = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<u16>>();
        return Ok(Some(PathBuf::from(OsString::from_wide(&wide))));
    }
    if encoding == "utf8-base64" {
        return String::from_utf8(bytes.to_vec())
            .map(|value| Some(PathBuf::from(value)))
            .map_err(|err| err.to_string());
    }
    Ok(None)
}

#[cfg(all(not(unix), not(windows)))]
fn path_from_native_bytes(encoding: &str, bytes: &[u8]) -> Result<Option<PathBuf>, String> {
    if encoding == "utf8-base64" {
        return String::from_utf8(bytes.to_vec())
            .map(|value| Some(PathBuf::from(value)))
            .map_err(|err| err.to_string());
    }
    Ok(None)
}

fn preflight_destinations(record: &SourceRecord, destination_dir: &Path) -> Result<(), String> {
    let mut seen_destinations = HashSet::with_capacity(record.items.len());

    for item in &record.items {
        let destination = destination_dir.join(final_component(&item.path)?);
        if fs::symlink_metadata(&destination).is_ok()
            || !seen_destinations.insert(destination.clone())
        {
            return Err(format!(
                "fpaste: error: same file name already exists: {}\nfpaste: paste stopped before changing files\nTo overwrite, use freplace instead",
                destination.display()
            ));
        }
    }
    Ok(())
}

fn final_component(path: &Path) -> Result<&OsStr, String> {
    path.file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            format!(
                "fpaste: cannot reconstruct path for item: no final component in {}",
                path.display()
            )
        })
}

fn paste_item(operation: PasteOperation, source: &Path, destination: &Path) -> std::io::Result<()> {
    if operation.is_copy() {
        copy_path(source, destination)
    } else {
        move_path(source, destination)
    }
}

fn copy_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        copy_symlink(source, destination)
    } else if metadata.is_dir() {
        copy_dir_recursive(source, destination)
    } else {
        fs::copy(source, destination).map(|_| ())
    }
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> std::io::Result<()> {
    let target = fs::read_link(source)?;
    let resolved_target = if target.is_absolute() {
        target.clone()
    } else {
        source
            .parent()
            .map(|parent| parent.join(&target))
            .unwrap_or_else(|| target.clone())
    };

    if fs::metadata(&resolved_target)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
}

#[cfg(all(not(unix), not(windows)))]
fn copy_symlink(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "copying symlinks is not supported on this platform",
    ))
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_child = entry.path();
        let destination_child = destination.join(entry.file_name());
        copy_path(&source_child, &destination_child)?;
    }
    Ok(())
}

fn move_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            copy_path(source, destination)?;
            let metadata = fs::symlink_metadata(source)?;
            let remove_result = if metadata.is_dir() {
                fs::remove_dir_all(source)
            } else {
                fs::remove_file(source)
            };
            remove_result.map_err(|_| rename_err)
        }
    }
}

fn append_paste_history(
    history: &Path,
    record: &SourceRecord,
    destination_dir: &Path,
    outcomes: &[PasteOutcome],
) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(false)
        .open(history)
        .map_err(|err| format!("fpaste: cannot open history {}: {err}", history.display()))?;

    file.lock_exclusive()
        .map_err(|err| format!("fpaste: cannot open history {}: {err}", history.display()))?;

    let write_result = (|| {
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let line = paste_history_line(record, destination_dir, outcomes);
        let updated = capped_history_contents(&contents, &line);
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()
    })();

    let unlock_result = file.unlock();
    write_result
        .map_err(|err| format!("fpaste: cannot write history {}: {err}", history.display()))?;
    unlock_result
        .map_err(|err| format!("fpaste: cannot write history {}: {err}", history.display()))?;
    Ok(())
}

fn paste_history_line(
    record: &SourceRecord,
    destination_dir: &Path,
    outcomes: &[PasteOutcome],
) -> String {
    let now = SystemTime::now();
    let destination_encoded = encode_native_path(destination_dir.as_os_str());
    let failed_count = outcomes
        .iter()
        .filter(|outcome| outcome.status != ItemStatus::Pasted)
        .count();

    let mut json = String::new();
    json.push_str("{\"id\":");
    push_json_string(&mut json, &unique_id(now));
    json.push_str(",\"operation\":\"paste\",\"created_at\":");
    push_json_string(&mut json, &utc_timestamp(now));
    json.push_str(",\"source_host\":");
    push_json_string(&mut json, &record.source_host);
    json.push_str(",\"source_user\":");
    push_json_string(&mut json, &record.source_user);
    json.push_str(",\"destination_host\":");
    push_json_string(&mut json, &source_host());
    json.push_str(",\"destination_user\":");
    push_json_string(&mut json, &source_user());
    json.push_str(",\"source_record_id\":");
    push_json_string(&mut json, &record.id);
    json.push_str(",\"source_operation\":");
    push_json_string(&mut json, record.operation.as_str());
    json.push_str(",\"destination_dir\":");
    push_json_string(&mut json, &destination_dir.display().to_string());
    json.push_str(",\"destination_dir_encoding\":");
    push_json_string(&mut json, destination_encoded.encoding);
    json.push_str(",\"destination_dir_data\":");
    push_json_string(&mut json, &destination_encoded.data);
    json.push_str(",\"failed_count\":");
    json.push_str(&failed_count.to_string());

    json.push_str(",\"items\":[");
    let mut first = true;
    for outcome in outcomes
        .iter()
        .filter(|outcome| outcome.status == ItemStatus::Pasted)
    {
        if !first {
            json.push(',');
        }
        first = false;
        push_outcome_item(&mut json, outcome, "pasted", None);
    }
    json.push(']');

    json.push_str(",\"failed_items\":[");
    let mut first = true;
    for outcome in outcomes
        .iter()
        .filter(|outcome| outcome.status != ItemStatus::Pasted)
    {
        if !first {
            json.push(',');
        }
        first = false;
        let error = match &outcome.status {
            ItemStatus::Failed(error) => Some(error.as_str()),
            ItemStatus::Pasted => None,
        };
        push_outcome_item(&mut json, outcome, "failed", error);
    }
    json.push_str("]}");
    json
}

fn push_outcome_item(json: &mut String, outcome: &PasteOutcome, status: &str, error: Option<&str>) {
    let destination_encoded = encode_native_path(outcome.destination.as_os_str());
    json.push_str("{\"source_path\":");
    push_json_string(json, &outcome.source.display_path);
    if let Some(encoding) = &outcome.source.path_encoding {
        json.push_str(",\"source_path_encoding\":");
        push_json_string(json, encoding);
    }
    if let Some(data) = &outcome.source.path_data {
        json.push_str(",\"source_path_data\":");
        push_json_string(json, data);
    }
    json.push_str(",\"destination_path\":");
    push_json_string(json, &outcome.destination.display().to_string());
    json.push_str(",\"destination_path_encoding\":");
    push_json_string(json, destination_encoded.encoding);
    json.push_str(",\"destination_path_data\":");
    push_json_string(json, &destination_encoded.data);
    json.push_str(",\"file_type\":");
    push_json_string(json, &outcome.source.file_type);
    json.push_str(",\"status\":");
    push_json_string(json, status);
    if let Some(error) = error {
        json.push_str(",\"error\":");
        push_json_string(json, error);
    }
    json.push('}');
}

fn capped_history_contents(existing_contents: &str, new_line: &str) -> String {
    let existing_count = existing_contents.lines().count();
    let skip_count = existing_count
        .saturating_add(1)
        .saturating_sub(MAX_HISTORY_RECORDS);
    let mut lines = existing_contents
        .lines()
        .skip(skip_count)
        .chain(std::iter::once(new_line));
    let mut contents = String::new();
    if let Some(line) = lines.next() {
        contents.push_str(line);
        for line in lines {
            contents.push('\n');
            contents.push_str(line);
        }
        contents.push('\n');
    }
    contents
}

fn encode_native_path(path: &OsStr) -> EncodedPath {
    let (encoding, bytes) = native_path_bytes(path);
    EncodedPath {
        encoding,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

#[cfg(unix)]
fn native_path_bytes(path: &OsStr) -> (&'static str, Vec<u8>) {
    use std::os::unix::ffi::OsStrExt;
    ("unix-bytes-base64", path.as_bytes().to_vec())
}

#[cfg(windows)]
fn native_path_bytes(path: &OsStr) -> (&'static str, Vec<u8>) {
    use std::os::windows::ffi::OsStrExt;
    (
        "windows-utf16le-base64",
        path.encode_wide().flat_map(u16::to_le_bytes).collect(),
    )
}

#[cfg(all(not(unix), not(windows)))]
fn native_path_bytes(path: &OsStr) -> (&'static str, Vec<u8>) {
    ("utf8-base64", path.to_string_lossy().as_bytes().to_vec())
}

fn unique_id(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!(
        "{}-{}-{}",
        duration.as_secs(),
        duration.subsec_nanos(),
        process::id()
    )
}

fn utc_timestamp(time: SystemTime) -> String {
    let Ok(duration) = time.duration_since(UNIX_EPOCH) else {
        return "1970-01-01T00:00:00Z".to_string();
    };
    let total_seconds = duration.as_secs();
    let days = (total_seconds / 86_400) as i64;
    let seconds_of_day = total_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

fn push_json_string(json: &mut String, value: &str) {
    json.push('"');
    for ch in value.chars() {
        match ch {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\u{08}' => json.push_str("\\b"),
            '\u{0c}' => json.push_str("\\f"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            ch if ch < ' ' => json.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => json.push(ch),
        }
    }
    json.push('"');
}

fn source_host() -> String {
    non_empty_env("HOSTNAME")
        .or_else(|| non_empty_env("COMPUTERNAME"))
        .or_else(system_hostname)
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(unix)]
fn system_hostname() -> Option<String> {
    fs::read_to_string("/etc/hostname")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(not(unix))]
fn system_hostname() -> Option<String> {
    None
}

fn source_user() -> String {
    non_empty_env("USER")
        .or_else(|| non_empty_env("USERNAME"))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(windows)]
fn history_path() -> Result<PathBuf, String> {
    if let Some(value) = non_empty_env("LOCALAPPDATA") {
        return Ok(PathBuf::from(value).join("fileclip").join("history.jsonl"));
    }
    if let Some(value) = non_empty_env("USERPROFILE") {
        return Ok(PathBuf::from(value)
            .join("AppData")
            .join("Local")
            .join("fileclip")
            .join("history.jsonl"));
    }
    Err(
        "fpaste: cannot determine history directory: LOCALAPPDATA and USERPROFILE are not set"
            .to_string(),
    )
}

#[cfg(target_os = "macos")]
fn history_path() -> Result<PathBuf, String> {
    let home = non_empty_env("HOME")
        .ok_or_else(|| "fpaste: cannot determine history directory: HOME is not set".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("fileclip")
        .join("history.jsonl"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn history_path() -> Result<PathBuf, String> {
    if let Some(value) = non_empty_env("XDG_STATE_HOME") {
        return Ok(PathBuf::from(value).join("fileclip").join("history.jsonl"));
    }
    let home = non_empty_env("HOME")
        .ok_or_else(|| "fpaste: cannot determine history directory: HOME is not set".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("fileclip")
        .join("history.jsonl"))
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_record(paths: &[&str]) -> SourceRecord {
        SourceRecord {
            id: "test-record".to_string(),
            operation: PasteOperation::Copy,
            source_host: "test-host".to_string(),
            source_user: "test-user".to_string(),
            items: paths.iter().map(|path| source_item(path)).collect(),
        }
    }

    fn source_item(path: &str) -> SourceItem {
        SourceItem {
            path: PathBuf::from(path),
            display_path: path.to_string(),
            path_encoding: None,
            path_data: None,
            file_type: "file".to_string(),
        }
    }

    #[test]
    fn latest_supported_record_prefers_newer_copy_dir_over_older_move() {
        let contents = concat!(
            "{\"id\":\"old-move\",\"operation\":\"move\",\"items\":[{\"path\":\"/old/starship.toml\",\"file_type\":\"file\"}]}\n",
            "{\"id\":\"new-copy\",\"operation\":\"copy\",\"items\":[{\"path\":\"/project/icons\",\"file_type\":\"dir\"}]}\n",
        );

        let record = latest_supported_record(contents).expect("latest supported record");

        assert_eq!(record.id, "new-copy");
        assert!(record.operation.is_copy());
        assert_eq!(record.items[0].path, PathBuf::from("/project/icons"));
        assert_eq!(record.items[0].file_type, "dir");
    }

    #[test]
    fn latest_supported_record_skips_paste_result_and_uses_newer_copy_dir() {
        let contents = concat!(
            "{\"id\":\"old-move\",\"operation\":\"move\",\"items\":[{\"path\":\"/old/starship.toml\",\"file_type\":\"file\"}]}\n",
            "{\"id\":\"new-copy\",\"operation\":\"copy\",\"items\":[{\"path\":\"/project/icons\",\"file_type\":\"dir\"}]}\n",
            "{\"id\":\"paste-result\",\"operation\":\"paste\",\"source_record_id\":\"new-copy\",\"source_operation\":\"copy\",\"items\":[]}\n",
        );

        let record = latest_supported_record(contents).expect("latest supported record");

        assert_eq!(record.id, "new-copy");
        assert!(record.operation.is_copy());
        assert_eq!(record.items[0].path, PathBuf::from("/project/icons"));
        assert_eq!(record.items[0].file_type, "dir");
    }

    #[test]
    fn preflight_destinations_rejects_duplicate_final_components() {
        let record = source_record(&["/source/one/report.txt", "/source/two/report.txt"]);
        let destination_dir = PathBuf::from("/destination/does/not/exist");

        let result = preflight_destinations(&record, &destination_dir);

        let error = result.unwrap_err();
        assert!(error.contains("fpaste: error: same file name already exists:"));
        assert!(error.contains("To overwrite, use freplace instead"));
    }

    #[test]
    fn preflight_destinations_allows_distinct_final_components() {
        let record = source_record(&["/source/one/report.txt", "/source/two/notes.txt"]);
        let destination_dir = PathBuf::from("/destination/does/not/exist");

        let result = preflight_destinations(&record, &destination_dir);

        assert_eq!(result, Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn preflight_destinations_rejects_broken_destination_symlink() {
        use std::os::unix::fs::symlink;

        let test_dir = unique_test_dir("preflight-broken-destination-symlink");
        fs::create_dir(&test_dir).expect("create test dir");
        symlink("missing-target", test_dir.join("report.txt")).expect("create broken symlink");
        let record = source_record(&["/source/report.txt"]);

        let result = preflight_destinations(&record, &test_dir);

        let error = result.unwrap_err();
        assert!(error.contains("fpaste: error: same file name already exists:"));
        assert!(error.contains("To overwrite, use freplace instead"));
        fs::remove_dir_all(&test_dir).expect("remove test dir");
    }

    #[cfg(unix)]
    #[test]
    fn copy_path_recreates_symlink_without_dereferencing() {
        use std::os::unix::fs::symlink;

        let test_dir = unique_test_dir("copy-symlink");
        fs::create_dir(&test_dir).expect("create test dir");
        fs::write(test_dir.join("target.txt"), "target contents").expect("write target");
        let source = test_dir.join("source-link");
        let destination = test_dir.join("destination-link");
        symlink("target.txt", &source).expect("create symlink");

        copy_path(&source, &destination).expect("copy symlink");

        assert!(
            fs::symlink_metadata(&destination)
                .expect("destination metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_link(&destination).expect("read link"),
            PathBuf::from("target.txt")
        );
        fs::remove_dir_all(&test_dir).expect("remove test dir");
    }

    #[cfg(unix)]
    #[test]
    fn copy_path_recreates_broken_symlink() {
        use std::os::unix::fs::symlink;

        let test_dir = unique_test_dir("copy-broken-symlink");
        fs::create_dir(&test_dir).expect("create test dir");
        let source = test_dir.join("source-link");
        let destination = test_dir.join("destination-link");
        symlink("missing-target", &source).expect("create broken symlink");

        copy_path(&source, &destination).expect("copy broken symlink");

        assert!(
            fs::symlink_metadata(&destination)
                .expect("destination metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_link(&destination).expect("read link"),
            PathBuf::from("missing-target")
        );
        fs::remove_dir_all(&test_dir).expect("remove test dir");
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_preserves_symlink_children() {
        use std::os::unix::fs::symlink;

        let test_dir = unique_test_dir("copy-dir-symlink-child");
        let source_dir = test_dir.join("source");
        let destination_dir = test_dir.join("destination");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::write(source_dir.join("target.txt"), "target contents").expect("write target");
        symlink("target.txt", source_dir.join("child-link")).expect("create symlink");

        copy_path(&source_dir, &destination_dir).expect("copy dir");

        let destination_link = destination_dir.join("child-link");
        assert!(
            fs::symlink_metadata(&destination_link)
                .expect("destination metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_link(&destination_link).expect("read link"),
            PathBuf::from("target.txt")
        );
        fs::remove_dir_all(&test_dir).expect("remove test dir");
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "ftools-fpaste-{name}-{}",
            unique_id(SystemTime::now())
        ))
    }

    #[cfg(unix)]
    #[test]
    fn reconstruct_path_decodes_unix_bytes_base64() {
        use std::os::unix::ffi::OsStrExt;

        let path = reconstruct_path(
            Some("unix-bytes-base64"),
            Some(&base64::engine::general_purpose::STANDARD.encode(b"/tmp/non-utf8-\xff")),
            None,
        )
        .unwrap();

        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/non-utf8-\xff");
    }

    #[cfg(windows)]
    #[test]
    fn reconstruct_path_decodes_windows_utf16le_base64() {
        let bytes = "C:\\Temp\\report.txt"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>();

        let path = reconstruct_path(
            Some("windows-utf16le-base64"),
            Some(&base64::engine::general_purpose::STANDARD.encode(bytes)),
            None,
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("C:\\Temp\\report.txt"));
    }

    #[cfg(windows)]
    #[test]
    fn reconstruct_path_rejects_malformed_windows_utf16le_base64() {
        let result = reconstruct_path(
            Some("windows-utf16le-base64"),
            Some(&base64::engine::general_purpose::STANDARD.encode([0x61])),
            None,
        );

        assert_eq!(result, Err("odd number of UTF-16 bytes".to_string()));
    }

    #[test]
    fn reconstruct_path_rejects_malformed_base64() {
        let result = reconstruct_path(Some("utf8-base64"), Some("not valid base64"), None);

        assert!(result.is_err());
    }
}
