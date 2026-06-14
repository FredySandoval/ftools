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

pub fn fdelete_main() -> Result<(), String> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.is_empty() {
        return Err("fdelete: nothing to delete".to_string());
    }

    let mut planned = Vec::with_capacity(args.len());
    for arg in &args {
        let path = PathBuf::from(arg);
        validate_delete_target(&path)?;
        let original = absolute_path_preserving_final_symlink(&path)
            .map_err(|e| format!("fdelete: cannot resolve {}: {e}", path.display()))?;
        validate_delete_target(&original)?;
        let meta = fs::symlink_metadata(&original)
            .map_err(|e| format!("fdelete: cannot access {}: {e}", original.display()))?;
        let name = final_component(&original)?.to_os_string();
        planned.push((original, file_type_name(&meta).to_string(), name));
    }

    let trash = env::temp_dir().join(format!("fileclip-delete-{}", unique_id(SystemTime::now())));
    fs::create_dir_all(&trash)
        .map_err(|e| format!("fdelete: cannot create temp dir {}: {e}", trash.display()))?;

    let mut items = Vec::with_capacity(planned.len());
    for (index, (original, file_type, name)) in planned.into_iter().enumerate() {
        let dest = trash.join(format!("{}-{}", index, name.to_string_lossy()));
        move_path(&original, &dest)
            .map_err(|e| format!("fdelete: cannot move {} to temp: {e}", original.display()))?;
        println!(
            "deleted to temp: {} -> {}",
            original.display(),
            dest.display()
        );
        items.push(DeleteItem {
            temp: dest,
            original,
            file_type,
        });
    }
    append_line("fdelete", &delete_history_line(&items))
}

pub fn freplace_main() -> Result<(), String> {
    if env::args_os().len() > 1 {
        return Err("freplace: does not accept arguments".to_string());
    }
    let record = latest_source_record("freplace")?;
    let cwd = env::current_dir()
        .map_err(|e| format!("freplace: cannot determine current directory: {e}"))?;
    let temp = env::temp_dir().join(format!("fileclip-replace-{}", unique_id(SystemTime::now())));
    fs::create_dir_all(&temp)
        .map_err(|e| format!("freplace: cannot create temp dir {}: {e}", temp.display()))?;
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for item in record.items.clone() {
        let dest = match final_component(&item.path) {
            Ok(name) => cwd.join(name),
            Err(e) => {
                failed.push(FailedItem::new(item.path, e));
                continue;
            }
        };
        let displaced = if fs::symlink_metadata(&dest).is_ok() {
            let d = temp.join(format!(
                "{}-{}",
                done.len() + failed.len(),
                dest.file_name().unwrap_or_default().to_string_lossy()
            ));
            match move_path(&dest, &d) {
                Ok(()) => {
                    println!(
                        "moved existing destination to temp: {} -> {}",
                        dest.display(),
                        d.display()
                    );
                    Some(d)
                }
                Err(e) => {
                    eprintln!(
                        "freplace: cannot move existing destination {} to {}: {e}",
                        dest.display(),
                        d.display()
                    );
                    failed.push(FailedItem::new(item.path, e.to_string()));
                    continue;
                }
            }
        } else {
            None
        };
        if let Err(e) = paste_item(record.operation.as_deref(), &item.path, &dest) {
            if let Some(d) = &displaced {
                let _ = move_path(d, &dest);
            }
            failed.push(FailedItem::new(item.path, e.to_string()));
        } else {
            println!("replaced: {} -> {}", item.path.display(), dest.display());
            done.push(ReplaceItem {
                source: item.path,
                destination: dest,
                displaced,
                file_type: item.file_type,
            });
        }
    }
    append_line(
        "freplace",
        &replace_history_line(&record, &cwd, &done, &failed),
    )?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err("freplace: one or more items failed".to_string())
    }
}

pub fn fundo_main() -> Result<(), String> {
    if env::args_os().len() > 1 {
        return Err("fundo: does not accept arguments".to_string());
    }
    let rec = latest_undoable_record("fundo")?;
    let (done, failed) = undo_record(&rec, false);
    append_line("fundo", &move_history_line("undo", &rec, &done, &failed))?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err("fundo: one or more items failed".to_string())
    }
}

pub fn fredo_main() -> Result<(), String> {
    if env::args_os().len() > 1 {
        return Err("fredo: does not accept arguments".to_string());
    }
    let rec = latest_redoable_record("fredo")?;
    let (done, failed) = if rec.op == "undo" {
        redo_undo(&rec)
    } else {
        redo_record(&rec)
    };
    append_line("fredo", &move_history_line("redo", &rec, &done, &failed))?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err("fredo: one or more items failed".to_string())
    }
}

#[derive(Clone)]
struct SourceItem {
    path: PathBuf,
    file_type: Option<String>,
}
struct SourceRecord {
    id: Option<String>,
    operation: Option<String>,
    items: Vec<SourceItem>,
}
struct DeleteItem {
    temp: PathBuf,
    original: PathBuf,
    file_type: String,
}
struct ReplaceItem {
    source: PathBuf,
    destination: PathBuf,
    displaced: Option<PathBuf>,
    file_type: Option<String>,
}
struct FailedItem {
    path: PathBuf,
    error: String,
}
impl FailedItem {
    fn new(path: PathBuf, error: String) -> Self {
        Self { path, error }
    }
}
#[derive(Clone)]
struct MoveItem {
    from: PathBuf,
    to: PathBuf,
    file_type: Option<String>,
}
#[derive(Clone)]
struct AnyRecord {
    id: Option<String>,
    op: String,
    source_record_id: Option<String>,
    source_operation: Option<String>,
    redone_operation: Option<String>,
    items: Vec<Value>,
}
struct EncodedPath {
    encoding: &'static str,
    data: String,
}

fn validate_delete_target(path: &Path) -> Result<(), String> {
    let s = path.as_os_str().to_string_lossy();
    if s.is_empty()
        || path == Path::new("/")
        || path == Path::new(".")
        || path == Path::new("..")
        || s == "~"
        || s.ends_with("/*")
        || s == "*"
    {
        return Err(format!(
            "fdelete: refusing broad or dangerous target {}; use rm instead",
            path.display()
        ));
    }
    Ok(())
}

fn latest_source_record(cmd: &str) -> Result<SourceRecord, String> {
    let contents = fs::read_to_string(history_path()?)
        .map_err(|e| format!("{cmd}: cannot read history: {e}"))?;
    for line in contents.lines().rev().filter(|l| !l.trim().is_empty()) {
        let v: Value =
            serde_json::from_str(line).map_err(|e| format!("{cmd}: malformed history: {e}"))?;
        let Some(obj) = v.as_object() else {
            continue;
        };
        let op = string_field(obj, "operation");
        if !matches!(op.as_deref(), Some("copy" | "move" | "cut" | "delete")) {
            continue;
        }
        let mut items = Vec::new();
        for it in obj
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(io) = it.as_object()
                && let Ok(p) = item_path(io, "path")
            {
                items.push(SourceItem {
                    path: p,
                    file_type: string_field(io, "file_type"),
                });
            }
        }
        if !items.is_empty() {
            return Ok(SourceRecord {
                id: string_field(obj, "id"),
                operation: op,
                items,
            });
        }
    }
    Err(format!("{cmd}: no supported history records"))
}

fn latest_redoable_record(cmd: &str) -> Result<AnyRecord, String> {
    let records = history_records(cmd)?;
    let mut redone_undos = std::collections::HashSet::new();
    for rec in &records {
        if rec.op == "redo"
            && rec.source_operation.as_deref() == Some("undo")
            && let Some(id) = &rec.source_record_id
        {
            redone_undos.insert(id.clone());
        }
    }
    for rec in records.iter().rev() {
        if rec.op == "undo" {
            if rec.id.as_ref().is_none_or(|id| !redone_undos.contains(id)) {
                return Ok(rec.clone());
            }
        } else if matches!(rec.op.as_str(), "delete" | "paste" | "replace") {
            return Ok(rec.clone());
        }
    }
    Err(format!("{cmd}: no redoable history records"))
}

fn latest_undoable_record(cmd: &str) -> Result<AnyRecord, String> {
    let records = history_records(cmd)?;
    let mut undone = std::collections::HashSet::new();
    for rec in &records {
        if rec.op == "undo" {
            if let Some(id) = &rec.source_record_id {
                undone.insert(id.clone());
            }
        } else if rec.op == "redo"
            && let Some(id) = &rec.source_record_id
            && let Some(undo_rec) = records
                .iter()
                .find(|candidate| candidate.id.as_ref() == Some(id))
            && let Some(source_id) = &undo_rec.source_record_id
        {
            undone.remove(source_id);
        }
    }
    for rec in records.iter().rev() {
        if matches!(rec.op.as_str(), "delete" | "paste" | "replace" | "redo")
            && rec.id.as_ref().is_none_or(|id| !undone.contains(id))
        {
            return Ok(rec.clone());
        }
    }
    Err(format!("{cmd}: no undoable history records"))
}

fn history_records(cmd: &str) -> Result<Vec<AnyRecord>, String> {
    let contents = fs::read_to_string(history_path()?)
        .map_err(|e| format!("{cmd}: cannot read history: {e}"))?;
    let mut records = Vec::new();
    for line in contents.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value =
            serde_json::from_str(line).map_err(|e| format!("{cmd}: malformed history: {e}"))?;
        if let Some(obj) = v.as_object()
            && let Some(op) = string_field(obj, "operation")
        {
            let items = obj
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            records.push(AnyRecord {
                id: string_field(obj, "id"),
                op,
                source_record_id: string_field(obj, "source_record_id"),
                source_operation: string_field(obj, "source_operation"),
                redone_operation: string_field(obj, "redone_operation"),
                items,
            });
        }
    }
    Ok(records)
}

fn undo_record(rec: &AnyRecord, _redo: bool) -> (Vec<MoveItem>, Vec<FailedItem>) {
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for v in &rec.items {
        if let Some(o) = v.as_object() {
            match rec.op.as_str() {
                "delete" => apply_pair(
                    (item_path(o, "path"), item_path(o, "original_path")),
                    &mut done,
                    &mut failed,
                ),
                "paste" => undo_paste_item(o, &mut done, &mut failed),
                "replace" => undo_replace_item(o, &mut done, &mut failed),
                "redo" => apply_pair(
                    (item_path(o, "to_path"), item_path(o, "from_path")),
                    &mut done,
                    &mut failed,
                ),
                _ => {}
            }
        }
    }
    (done, failed)
}

fn undo_paste_item(o: &Map<String, Value>, done: &mut Vec<MoveItem>, failed: &mut Vec<FailedItem>) {
    let destination = match item_path(o, "destination_path") {
        Ok(path) => path,
        Err(error) => {
            failed.push(FailedItem::new(PathBuf::new(), error));
            return;
        }
    };
    let source = item_path(o, "source_path").unwrap_or_else(|_| destination.clone());
    if fs::symlink_metadata(&source).is_ok() {
        match remove_path(&destination) {
            Ok(()) => {
                println!("undone: {} -> {}", destination.display(), source.display());
                done.push(MoveItem {
                    from: destination,
                    to: source,
                    file_type: string_field(o, "file_type"),
                })
            }
            Err(error) => failed.push(FailedItem::new(destination, error.to_string())),
        }
    } else {
        apply_pair((Ok(destination), Ok(source)), done, failed);
    }
}

fn undo_replace_item(
    o: &Map<String, Value>,
    done: &mut Vec<MoveItem>,
    failed: &mut Vec<FailedItem>,
) {
    let destination = match item_path(o, "destination_path") {
        Ok(path) => path,
        Err(error) => {
            failed.push(FailedItem::new(PathBuf::new(), error));
            return;
        }
    };
    if let Err(error) = remove_path(&destination) {
        failed.push(FailedItem::new(destination, error.to_string()));
        return;
    }
    if let Ok(displaced) = item_path(o, "displaced_path") {
        apply_pair(
            (Ok(displaced), item_path(o, "original_destination_path")),
            done,
            failed,
        );
    } else {
        println!(
            "undone: {} -> {}",
            destination.display(),
            destination.display()
        );
        done.push(MoveItem {
            from: destination.clone(),
            to: destination,
            file_type: string_field(o, "file_type"),
        });
    }
}
fn redo_undo(rec: &AnyRecord) -> (Vec<MoveItem>, Vec<FailedItem>) {
    let mut d = Vec::new();
    let mut f = Vec::new();
    for v in &rec.items {
        if let Some(o) = v.as_object() {
            apply_pair_named(
                (item_path(o, "to_path"), item_path(o, "from_path")),
                &mut d,
                &mut f,
                "fredo",
                "redone",
            );
        }
    }
    (d, f)
}
fn redo_record(rec: &AnyRecord) -> (Vec<MoveItem>, Vec<FailedItem>) {
    let mut d = Vec::new();
    let mut f = Vec::new();
    for v in &rec.items {
        if let Some(o) = v.as_object() {
            let p = match rec.op.as_str() {
                "delete" => (item_path(o, "original_path"), item_path(o, "path")),
                "paste" => (
                    item_path(o, "source_path"),
                    item_path(o, "destination_path"),
                ),
                "replace" => (
                    item_path(o, "source_path"),
                    item_path(o, "destination_path"),
                ),
                _ => continue,
            };
            apply_pair_named(p, &mut d, &mut f, "fredo", "redone");
        }
    }
    (d, f)
}
fn apply_pair(
    pair: (Result<PathBuf, String>, Result<PathBuf, String>),
    done: &mut Vec<MoveItem>,
    failed: &mut Vec<FailedItem>,
) {
    apply_pair_named(pair, done, failed, "fundo", "undone");
}

fn apply_pair_named(
    pair: (Result<PathBuf, String>, Result<PathBuf, String>),
    done: &mut Vec<MoveItem>,
    failed: &mut Vec<FailedItem>,
    cmd: &str,
    verb: &str,
) {
    match pair {
        (Ok(from), Ok(to)) => match move_path(&from, &to) {
            Ok(()) => {
                println!("{verb}: {} -> {}", from.display(), to.display());
                done.push(MoveItem {
                    from,
                    to,
                    file_type: None,
                })
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("{cmd}: not found: {}", from.display());
                } else if cmd == "fredo" {
                    eprintln!("fredo: warning: could not redo {}: {e}", from.display());
                } else {
                    eprintln!(
                        "fundo: cannot undo {} to {}: {e}",
                        from.display(),
                        to.display()
                    );
                }
                failed.push(FailedItem::new(from, e.to_string()))
            }
        },
        (Ok(p), Err(e)) => failed.push(FailedItem::new(p, e)),
        _ => {}
    }
}

fn item_path(o: &Map<String, Value>, base: &str) -> Result<PathBuf, String> {
    reconstruct_path(
        string_field(o, &format!("{base}_encoding")).as_deref(),
        string_field(o, &format!("{base}_data")).as_deref(),
        string_field(o, base).as_deref(),
    )
}
fn string_field(o: &Map<String, Value>, name: &str) -> Option<String> {
    o.get(name).and_then(Value::as_str).map(ToOwned::to_owned)
}
fn reconstruct_path(
    enc: Option<&str>,
    data: Option<&str>,
    fallback: Option<&str>,
) -> Result<PathBuf, String> {
    if let (Some(e), Some(d)) = (enc, data) {
        let b = base64::engine::general_purpose::STANDARD
            .decode(d)
            .map_err(|e| e.to_string())?;
        if let Some(p) = path_from_native_bytes(e, &b)? {
            return Ok(p);
        }
    }
    fallback
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "missing path".to_string())
}
#[cfg(unix)]
fn path_from_native_bytes(e: &str, b: &[u8]) -> Result<Option<PathBuf>, String> {
    use std::os::unix::ffi::OsStringExt;
    if e == "unix-bytes-base64" {
        Ok(Some(PathBuf::from(OsString::from_vec(b.to_vec()))))
    } else if e == "utf8-base64" {
        String::from_utf8(b.to_vec())
            .map(|s| Some(PathBuf::from(s)))
            .map_err(|e| e.to_string())
    } else {
        Ok(None)
    }
}
#[cfg(windows)]
fn path_from_native_bytes(e: &str, b: &[u8]) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    if e == "windows-utf16le-base64" {
        if b.len() % 2 != 0 {
            return Err("odd number of UTF-16 bytes".into());
        }
        let w = b
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        Ok(Some(PathBuf::from(OsString::from_wide(&w))))
    } else if e == "utf8-base64" {
        String::from_utf8(b.to_vec())
            .map(|s| Some(PathBuf::from(s)))
            .map_err(|e| e.to_string())
    } else {
        Ok(None)
    }
}
#[cfg(all(not(unix), not(windows)))]
fn path_from_native_bytes(e: &str, b: &[u8]) -> Result<Option<PathBuf>, String> {
    if e == "utf8-base64" {
        String::from_utf8(b.to_vec())
            .map(|s| Some(PathBuf::from(s)))
            .map_err(|e| e.to_string())
    } else {
        Ok(None)
    }
}

fn paste_item(op: Option<&str>, source: &Path, dest: &Path) -> std::io::Result<()> {
    if matches!(op, Some("copy")) {
        copy_path(source, dest)
    } else {
        move_path(source, dest)
    }
}
fn copy_path(source: &Path, dest: &Path) -> std::io::Result<()> {
    let m = fs::symlink_metadata(source)?;
    if m.file_type().is_symlink() {
        copy_symlink(source, dest)
    } else if m.is_dir() {
        fs::create_dir(dest)?;
        for e in fs::read_dir(source)? {
            let e = e?;
            copy_path(&e.path(), &dest.join(e.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(source, dest).map(|_| ())
    }
}
#[cfg(unix)]
fn copy_symlink(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, dest)
}
#[cfg(windows)]
fn copy_symlink(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(fs::read_link(source)?, dest)
}
#[cfg(all(not(unix), not(windows)))]
fn copy_symlink(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("symlinks unsupported"))
}
fn move_path(source: &Path, dest: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(dest).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("destination exists: {}", dest.display()),
        ));
    }
    match fs::rename(source, dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            copy_path(source, dest)?;
            let m = fs::symlink_metadata(source)?;
            (if m.is_dir() {
                fs::remove_dir_all(source)
            } else {
                fs::remove_file(source)
            })
            .map_err(|_| e)
        }
    }
}
fn remove_path(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn final_component(path: &Path) -> Result<&OsStr, String> {
    path.file_name()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| format!("no final component in {}", path.display()))
}
fn absolute_path_preserving_final_symlink(path: &Path) -> std::io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}
fn file_type_name(m: &fs::Metadata) -> &'static str {
    if m.file_type().is_symlink() {
        "symlink"
    } else if m.is_dir() {
        "dir"
    } else if m.is_file() {
        "file"
    } else {
        "other"
    }
}

fn append_line(cmd: &str, line: &str) -> Result<(), String> {
    let h = history_path()?;
    if let Some(p) = h.parent() {
        fs::create_dir_all(p).map_err(|e| format!("{cmd}: cannot create history dir: {e}"))?;
    }
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&h)
        .map_err(|e| format!("{cmd}: cannot open history {}: {e}", h.display()))?;
    f.lock_exclusive().map_err(|e| e.to_string())?;
    let r = (|| {
        let mut c = String::new();
        f.read_to_string(&mut c)?;
        let u = capped_history_contents(&c, line);
        f.set_len(0)?;
        f.seek(SeekFrom::Start(0))?;
        f.write_all(u.as_bytes())?;
        f.sync_all()
    })();
    let _ = f.unlock();
    r.map_err(|e| format!("{cmd}: cannot write history: {e}"))
}
fn capped_history_contents(existing: &str, new_line: &str) -> String {
    let skip = existing
        .lines()
        .count()
        .saturating_add(1)
        .saturating_sub(MAX_HISTORY_RECORDS);
    let mut out = String::new();
    for l in existing.lines().skip(skip).chain(std::iter::once(new_line)) {
        out.push_str(l);
        out.push('\n');
    }
    out
}
fn delete_history_line(items: &[DeleteItem]) -> String {
    let mut j = prefix("delete");
    j.push_str(",\"source_host\":");
    push_json_string(&mut j, &source_host());
    j.push_str(",\"source_user\":");
    push_json_string(&mut j, &source_user());
    j.push_str(",\"items\":[");
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            j.push(',')
        }
        path_obj(&mut j, "path", &it.temp);
        j.pop();
        j.push_str(",\"original_path\":");
        push_json_string(&mut j, &it.original.display().to_string());
        add_encoded(&mut j, "original_path", &it.original);
        j.push_str(",\"file_type\":");
        push_json_string(&mut j, &it.file_type);
        j.push('}');
    }
    j.push_str("]}");
    j
}
fn replace_history_line(
    source: &SourceRecord,
    destination_dir: &Path,
    items: &[ReplaceItem],
    failed: &[FailedItem],
) -> String {
    let mut j = prefix("replace");
    add_source_fields(&mut j, source.id.as_deref(), source.operation.as_deref());
    field_path(&mut j, "destination_dir", destination_dir, true);
    j.push_str(",\"failed_count\":");
    j.push_str(&failed.len().to_string());
    j.push_str(",\"items\":[");
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            j.push(',')
        }
        j.push('{');
        field_path(&mut j, "source_path", &it.source, false);
        field_path(&mut j, "destination_path", &it.destination, true);
        field_path(&mut j, "original_destination_path", &it.destination, true);
        if let Some(d) = &it.displaced {
            field_path(&mut j, "displaced_path", d, true);
        }
        if let Some(file_type) = &it.file_type {
            j.push_str(",\"file_type\":");
            push_json_string(&mut j, file_type);
        }
        j.push_str(",\"status\":\"replaced\"}");
    }
    failed_json(&mut j, failed);
    j
}
fn move_history_line(
    op: &str,
    source: &AnyRecord,
    items: &[MoveItem],
    failed: &[FailedItem],
) -> String {
    let mut j = prefix(op);
    add_source_fields(&mut j, source.id.as_deref(), Some(&source.op));
    if op == "redo"
        && source.op == "undo"
        && let Some(redone) = source
            .source_operation
            .as_deref()
            .or(source.redone_operation.as_deref())
    {
        j.push_str(",\"redone_operation\":");
        push_json_string(&mut j, redone);
    }
    j.push_str(",\"failed_count\":");
    j.push_str(&failed.len().to_string());
    j.push_str(",\"items\":[");
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            j.push(',')
        }
        j.push('{');
        field_path(&mut j, "from_path", &it.from, false);
        field_path(&mut j, "to_path", &it.to, true);
        if let Some(file_type) = &it.file_type {
            j.push_str(",\"file_type\":");
            push_json_string(&mut j, file_type);
        }
        j.push_str(",\"status\":");
        push_json_string(&mut j, if op == "redo" { "redone" } else { "undone" });
        j.push('}');
    }
    failed_json(&mut j, failed);
    j
}
fn failed_json(j: &mut String, failed: &[FailedItem]) {
    j.push_str("],\"failed_items\":[");
    for (i, f) in failed.iter().enumerate() {
        if i > 0 {
            j.push(',')
        }
        j.push('{');
        field_path(j, "path", &f.path, false);
        j.push_str(",\"status\":\"failed\",\"error\":");
        push_json_string(j, &f.error);
        j.push('}');
    }
    j.push_str("]}");
}
fn prefix(op: &str) -> String {
    let now = SystemTime::now();
    let mut j = String::from("{\"id\":");
    push_json_string(&mut j, &unique_id(now));
    j.push_str(",\"operation\":");
    push_json_string(&mut j, op);
    j.push_str(",\"created_at\":");
    push_json_string(&mut j, &utc_timestamp(now));
    j
}
fn path_obj(j: &mut String, name: &str, path: &Path) {
    j.push('{');
    field_path(j, name, path, false);
    j.push('}');
}
fn field_path(j: &mut String, name: &str, path: &Path, comma: bool) {
    if comma {
        j.push(',')
    }
    j.push('"');
    j.push_str(name);
    j.push_str("\":");
    push_json_string(j, &path.display().to_string());
    add_encoded(j, name, path);
}
fn add_encoded(j: &mut String, name: &str, path: &Path) {
    let e = encode_native_path(path.as_os_str());
    j.push_str(",\"");
    j.push_str(name);
    j.push_str("_encoding\":");
    push_json_string(j, e.encoding);
    j.push_str(",\"");
    j.push_str(name);
    j.push_str("_data\":");
    push_json_string(j, &e.data);
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
fn push_json_string(j: &mut String, v: &str) {
    j.push('"');
    for ch in v.chars() {
        match ch {
            '"' => j.push_str("\\\""),
            '\\' => j.push_str("\\\\"),
            '\n' => j.push_str("\\n"),
            '\r' => j.push_str("\\r"),
            '\t' => j.push_str("\\t"),
            c if c < ' ' => j.push_str(&format!("\\u{:04x}", c as u32)),
            c => j.push(c),
        }
    }
    j.push('"');
}
fn unique_id(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}-{}-{}", d.as_secs(), d.subsec_nanos(), process::id())
}
fn utc_timestamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let (year, month, day) = civil_from_days(days);
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
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn add_source_fields(j: &mut String, id: Option<&str>, op: Option<&str>) {
    if let Some(id) = id {
        j.push_str(",\"source_record_id\":");
        push_json_string(j, id);
    }
    if let Some(op) = op {
        j.push_str(",\"source_operation\":");
        push_json_string(j, op);
    }
}

fn source_host() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| env::var("COMPUTERNAME").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn source_user() -> String {
    env::var("USER")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| env::var("USERNAME").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "unknown".to_string())
}
#[cfg(all(unix, not(target_os = "macos")))]
fn history_path() -> Result<PathBuf, String> {
    if let Ok(x) = env::var("XDG_STATE_HOME")
        && !x.is_empty()
    {
        return Ok(PathBuf::from(x).join("fileclip").join("history.jsonl"));
    }
    let h = env::var("HOME")
        .map_err(|_| "cannot determine history directory: HOME is not set".to_string())?;
    Ok(PathBuf::from(h).join(".local/state/fileclip/history.jsonl"))
}
#[cfg(target_os = "macos")]
fn history_path() -> Result<PathBuf, String> {
    Ok(
        PathBuf::from(env::var("HOME").map_err(|_| "HOME not set".to_string())?)
            .join("Library/Application Support/fileclip/history.jsonl"),
    )
}
#[cfg(windows)]
fn history_path() -> Result<PathBuf, String> {
    Ok(
        PathBuf::from(env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA not set".to_string())?)
            .join("fileclip/history.jsonl"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_root() {
        assert!(
            validate_delete_target(Path::new("/"))
                .unwrap_err()
                .contains("use rm instead")
        );
    }
    #[test]
    fn caps() {
        let s = (0..1001).map(|i| format!("{i}\n")).collect::<String>();
        assert_eq!(capped_history_contents(&s, "x").lines().count(), 1000);
    }
}
