use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use fs2::FileExt;

const MAX_HISTORY_RECORDS: usize = 1_000;

struct CutItem {
    path: PathBuf,
    file_type: &'static str,
}

struct EncodedPath {
    encoding: &'static str,
    data: String,
}

fn main() {
    match run() {
        Ok(_) => {}

        Err(message) => {
            eprintln!("{message}");
            process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return Err("fcut: nothing to cut".to_string());
    }

    let items = resolve_items(&args)?;
    append_history(&items)?;

    for item in items {
        println!("cut to clipboard: {}", item.path.display());
    }

    Ok(())
}

fn resolve_items(args: &[String]) -> Result<Vec<CutItem>, String> {
    let mut items = Vec::with_capacity(args.len());

    for arg in args {
        let path = PathBuf::from(arg);
        let absolute =
            absolute_path_preserving_final_symlink(&path).map_err(|err| match err.kind() {
                io::ErrorKind::NotFound => format!("fcut: not found: {arg}"),
                _ => format!("fcut: cannot resolve {arg}: {err}"),
            })?;
        let metadata = fs::symlink_metadata(&absolute).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => format!("fcut: not found: {arg}"),
            _ => format!(
                "fcut: cannot read metadata for {}: {err}",
                absolute.display()
            ),
        })?;

        items.push(CutItem {
            path: absolute,
            file_type: file_type_name(&metadata),
        });
    }

    Ok(items)
}

fn append_history(items: &[CutItem]) -> Result<(), String> {
    let history = history_path()?;
    let parent = history
        .parent()
        .ok_or_else(|| format!("fcut: invalid history path: {}", history.display()))?;

    fs::create_dir_all(parent).map_err(|err| {
        format!(
            "fcut: cannot create history directory {}: {err}",
            parent.display()
        )
    })?;

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&history)
        .map_err(|err| format!("fcut: cannot open history {}: {err}", history.display()))?;

    file.lock_exclusive()
        .map_err(|err| format!("fcut: cannot lock history {}: {err}", history.display()))?;

    let write_result = (|| {
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let updated_contents = capped_history_contents(&contents, &history_line(items));
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(updated_contents.as_bytes())?;
        file.sync_all()
    })();

    let unlock_result = file.unlock();

    write_result
        .map_err(|err| format!("fcut: cannot write history {}: {err}", history.display()))?;
    unlock_result
        .map_err(|err| format!("fcut: cannot unlock history {}: {err}", history.display()))?;

    Ok(())
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

fn history_line(items: &[CutItem]) -> String {
    let now = SystemTime::now();
    let id = unique_id(now);
    let created_at = utc_timestamp(now);
    let source_host = source_host();
    let source_user = source_user();

    let mut json = String::new();
    json.push_str("{\"id\":");
    push_json_string(&mut json, &id);
    json.push_str(",\"operation\":\"cut\",\"created_at\":");
    push_json_string(&mut json, &created_at);
    json.push_str(",\"source_host\":");
    push_json_string(&mut json, &source_host);
    json.push_str(",\"source_user\":");
    push_json_string(&mut json, &source_user);
    json.push_str(",\"items\":[");

    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        let encoded_path = encode_native_path(item.path.as_os_str());

        json.push_str("{\"path\":");
        push_json_string(&mut json, &item.path.display().to_string());
        json.push_str(",\"path_encoding\":");
        push_json_string(&mut json, encoded_path.encoding);
        json.push_str(",\"path_data\":");
        push_json_string(&mut json, &encoded_path.data);
        json.push_str(",\"file_type\":");
        push_json_string(&mut json, item.file_type);
        json.push('}');
    }

    json.push_str("]}");
    json
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

    let bytes = path
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<u8>>();

    ("windows-utf16le-base64", bytes)
}

#[cfg(all(not(unix), not(windows)))]
fn native_path_bytes(path: &OsStr) -> (&'static str, Vec<u8>) {
    ("utf8-base64", path.to_string_lossy().as_bytes().to_vec())
}

fn absolute_path_preserving_final_symlink(path: &Path) -> io::Result<PathBuf> {
    if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
        let absolute_parent = if parent.as_os_str().is_empty() {
            env::current_dir()?
        } else if parent.is_absolute() {
            fs::canonicalize(parent)?
        } else {
            fs::canonicalize(env::current_dir()?.join(parent))?
        };

        Ok(absolute_parent.join(file_name))
    } else if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        fs::canonicalize(env::current_dir()?.join(path))
    }
}

fn file_type_name(metadata: &fs::Metadata) -> &'static str {
    if metadata.file_type().is_symlink() {
        "symlink"
    } else if metadata.is_file() {
        "file"
    } else if metadata.is_dir() {
        "dir"
    } else {
        "other"
    }
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
        "fcut: cannot determine history directory: LOCALAPPDATA and USERPROFILE are not set"
            .to_string(),
    )
}

#[cfg(target_os = "macos")]
fn history_path() -> Result<PathBuf, String> {
    let home = non_empty_env("HOME")
        .ok_or_else(|| "fcut: cannot determine history directory: HOME is not set".to_string())?;

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
        .ok_or_else(|| "fcut: cannot determine history directory: HOME is not set".to_string())?;

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
    use std::time::Duration;

    use base64::Engine;

    #[test]
    fn push_json_string_escapes_quotes_backslashes_and_controls() {
        let mut json = String::new();

        push_json_string(
            &mut json,
            "quote \" backslash \\ newline\n carriage\r tab\t backspace\u{08} formfeed\u{0c} nul\u{00}",
        );

        assert_eq!(
            json,
            "\"quote \\\" backslash \\\\ newline\\n carriage\\r tab\\t backspace\\b formfeed\\f nul\\u0000\""
        );
    }

    #[test]
    fn utc_timestamp_formats_unix_epoch() {
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn utc_timestamp_formats_leap_year_date() {
        let time = UNIX_EPOCH + Duration::from_secs(1_582_934_400);

        assert_eq!(utc_timestamp(time), "2020-02-29T00:00:00Z");
    }

    #[test]
    fn utc_timestamp_formats_recent_fixed_timestamp() {
        let time = UNIX_EPOCH + Duration::from_secs(1_781_082_377);

        assert_eq!(utc_timestamp(time), "2026-06-10T09:06:17Z");
    }

    #[test]
    fn unique_id_uses_delimited_seconds_nanos_pid_string() {
        let time = UNIX_EPOCH + Duration::new(1_781_082_377, 25_753_875);
        let id = unique_id(time);
        let expected_prefix = "1781082377-25753875-";

        assert!(id.starts_with(expected_prefix), "id was {id}");
        assert_eq!(id.matches('-').count(), 2);
        assert!(
            id[expected_prefix.len()..]
                .chars()
                .all(|ch| ch.is_ascii_digit())
        );
    }

    #[test]
    fn capped_history_contents_appends_record() {
        let contents = capped_history_contents("one\ntwo\n", "three");

        assert_eq!(contents, "one\ntwo\nthree\n");
    }

    #[test]
    fn capped_history_contents_keeps_at_most_max_history_records() {
        let existing = (0..MAX_HISTORY_RECORDS)
            .map(|index| format!("old-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let contents = capped_history_contents(&existing, "new");
        let lines = contents.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), MAX_HISTORY_RECORDS);
        assert_eq!(lines.first(), Some(&"old-1"));
        assert_eq!(lines.last(), Some(&"new"));
    }

    #[test]
    fn capped_history_contents_drops_all_excess_old_records() {
        let existing = (0..(MAX_HISTORY_RECORDS + 25))
            .map(|index| format!("old-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let contents = capped_history_contents(&existing, "new");
        let lines = contents.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), MAX_HISTORY_RECORDS);
        assert_eq!(lines.first(), Some(&"old-26"));
        assert_eq!(lines.last(), Some(&"new"));
    }

    #[cfg(unix)]
    #[test]
    fn file_type_name_reports_symlink() {
        use std::fs::File;
        use std::os::unix::fs::symlink;

        let dir = env::temp_dir().join(format!("fcut-test-{}", unique_id(SystemTime::now())));
        fs::create_dir(&dir).expect("create temp test directory");
        let target = dir.join("target");
        let link = dir.join("link");
        File::create(&target).expect("create symlink target");
        symlink(&target, &link).expect("create symlink");

        let metadata = fs::symlink_metadata(&link).expect("read symlink metadata");

        assert_eq!(file_type_name(&metadata), "symlink");

        fs::remove_file(&link).expect("remove symlink");
        fs::remove_file(&target).expect("remove target");
        fs::remove_dir(&dir).expect("remove temp test directory");
    }

    #[test]
    fn history_line_uses_expected_shape_and_escapes_items() {
        let items = [CutItem {
            path: PathBuf::from("/tmp/file \"quoted\"\\name\n.txt"),
            file_type: "file",
        }];

        let line = history_line(&items);

        assert!(line.starts_with("{\"id\":\""), "line was {line}");
        assert!(
            line.contains("\",\"operation\":\"cut\","),
            "line was {line}"
        );
        assert!(
            line.contains("\"created_at\":\"") && line.contains("Z\",\"source_host\":"),
            "line was {line}"
        );
        assert!(line.contains("\"source_user\":"), "line was {line}");
        assert!(line.contains("\"items\":[{"), "line was {line}");
        assert!(
            line.contains("\"path\":\"/tmp/file \\\"quoted\\\"\\\\name\\n.txt\""),
            "line was {line}"
        );
        assert!(line.contains("\"path_encoding\":"), "line was {line}");
        assert!(line.contains("\"path_data\":"), "line was {line}");
        assert!(
            line.ends_with("\"file_type\":\"file\"}]}"),
            "line was {line}"
        );
    }

    #[test]
    fn encode_native_path_base64_encodes_regular_path() {
        let path = OsStr::new("abc");
        let encoded = encode_native_path(path);

        assert!(!encoded.data.is_empty());
        assert!(matches!(
            encoded.encoding,
            "unix-bytes-base64" | "windows-utf16le-base64" | "utf8-base64"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn encode_native_path_preserves_invalid_utf8_unix_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let bytes = vec![b'f', b'o', 0x80, b'o'];
        let path = std::ffi::OsString::from_vec(bytes.clone());
        let encoded = encode_native_path(path.as_os_str());

        assert_eq!(encoded.encoding, "unix-bytes-base64");
        assert_eq!(
            encoded.data,
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
    }
}
