FREPLACE(1)                 User Commands                 FREPLACE(1)

NAME
    freplace - paste fileclip items into the current directory, replacing collisions recoverably

SYNOPSIS
    freplace

DESCRIPTION
    freplace is similar to fpaste, but destination name collisions are replaced
    by default instead of causing the operation to abort.

    The history file is a JSON Lines file named history.jsonl in the platform
    fileclip state directory. The history has a hardcoded maximum of 1000
    records. Commands that append records delete the oldest records when needed
    so that history.jsonl keeps at most 1000 records.

    freplace must not be destructive. Before writing a pasted item over an
    existing destination name, freplace first moves the existing destination to
    the operating system temporary location, the same recovery area used by
    fdelete. The displaced destination is recorded in history so the replacement
    can be reversed with fundo while the temporary recovery item still exists.

OPTIONS
    freplace does not accept options.

ARGUMENTS
    freplace does not accept arguments.

BEHAVIOR
    freplace opens history.jsonl, searches from newest to oldest for the latest
    supported path-source operation, and applies it to the current working
    directory. Supported source operations are the same as fpaste: copy, move,
    cut, and delete.

    For each item, freplace determines the destination path as a child of the
    current working directory with the same final path component as the source
    item. If the destination does not exist, freplace behaves like fpaste for
    that item. If the destination exists, freplace first moves the existing
    destination to a unique temporary recovery path, records that displaced
    item, and then writes the new item at the destination.

    For copy records, source items are copied into place. For move, cut, and
    delete records, source items are moved into place. Directories are handled
    recursively. Symlinks should be preserved as symlinks rather than followed
    for their final component.

    If a source path is missing or an item fails after other items have been
    processed, freplace should warn the user, continue attempting independent
    remaining items when possible, and append a replace result record that
    includes successful items and failed items.

    Undoing a replace with fundo must first remove or move away the replacement
    destination at destination_path, then restore displaced_path to
    original_destination_path when displaced_path exists. History should
    represent both phases enough for audit; if only restoration is represented
    in undo items, replacement removal is still part of undo semantics and
    should be recorded with status/details where practical.

HISTORY FORMAT
    freplace reads the same path-source records as fpaste. After attempting a
    replacement, it appends a replace result record. Replace result records must
    include id, operation="replace", created_at, source_record_id,
    source_operation, destination_dir with native path encoding/data fields when
    available, failed_count, items, and failed_items. created_at is required and
    must be an RFC3339/ISO-8601 UTC timestamp in YYYY-MM-DDTHH:MM:SSZ format;
    epoch-like strings such as 1781233086Z are not valid.

    Each successful replace item must include source_path, destination_path,
    original_destination_path, native path encoding/data fields for those paths
    when available, file_type if known, and status="replaced". If a destination
    was displaced, displaced_path with encoding/data must be included. If no
    collision occurred, displaced_path is omitted or null. Failed items must be
    recorded in failed_items with status="failed" and an error.

    A replace result record uses this shape:

        {
          "id": "1781082700-333333333-335270",
          "operation": "replace",
          "created_at": "2026-06-10T09:11:40Z",
          "source_record_id": "1781082377-25753875-335232",
          "source_operation": "copy",
          "destination_dir": "/tmp/destination",
          "failed_count": 0,
          "items": [
            {
              "source_path": "/home/user/file1.txt",
              "destination_path": "/tmp/destination/file1.txt",
              "displaced_path": "/tmp/fileclip-1781082700/file1.txt",
              "original_destination_path": "/tmp/destination/file1.txt",
              "file_type": "file",
              "status": "replaced"
            }
          ],
          "failed_items": []
        }

    If no destination was displaced for an item, displaced_path may be omitted
    or null. Implementations should include native path encoding fields such as
    source_path_encoding, destination_path_encoding, and displaced_path_encoding
    when exact platform path reconstruction is needed.

FILES
    Linux and other Unix systems:
        $XDG_STATE_HOME/fileclip/history.jsonl
        ~/.local/state/fileclip/history.jsonl

    macOS:
        ~/Library/Application Support/fileclip/history.jsonl

    Windows:
        %LOCALAPPDATA%\fileclip\history.jsonl
        %USERPROFILE%\AppData\Local\fileclip\history.jsonl

OUTPUT
    Successful replacement must print one per-item message. The required form
    is:

        replaced: /home/user/file1.txt -> ./file1.txt

    When an existing destination is displaced, freplace must print one
    per-item message. The required form is:

        moved existing destination to temp: ./file1.txt -> /tmp/fileclip-1781082700/file1.txt

DIAGNOSTICS
    freplace: history not found: PATH
        The fileclip history file does not exist.

    freplace: no supported history records
        No readable history record with a supported path-source operation was
        found.

    freplace: not found: PATH
        A recorded source path does not exist at replace time.

    freplace: cannot move existing destination DESTINATION to TEMP: ERROR
        A colliding destination could not be moved to temporary recovery
        storage, so it must not be overwritten.

    freplace: cannot copy SOURCE to DESTINATION: ERROR
        Copying a source item failed.

    freplace: cannot move SOURCE to DESTINATION: ERROR
        Moving a source item failed.

    freplace: cannot write history PATH: ERROR
        The replace result record could not be appended or synced.

EXIT STATUS
    0
        The command completed successfully and every item was placed into the
        current working directory.

    1
        The command failed, or one or more items could not be replaced.

EXAMPLES
    Replace an existing destination using the latest fcopy record:

        fcopy ~/notes.txt
        cd /tmp/destination
        freplace

    Undo a replacement:

        fundo

    Recover a deleted item into the current directory, replacing any collision
    recoverably:

        fdelete old-report.txt
        cd ~/reports
        freplace

LIMITATIONS
    freplace depends on source paths and displaced temporary recovery paths
    remaining accessible. The operating system may delete temporary files before
    fundo can restore them.

    freplace replaces name collisions; it does not merge directory contents
    unless an installed implementation explicitly documents merge behavior.

    Exact native path reconstruction depends on path_encoding/path_data fields.
    Display path fields may be lossy on platforms where native paths are not
    always UTF-8.

DESIGN NOTES
    freplace exists for the common case where overwrite behavior is desired but
    permanent destruction is not. Existing destination names are moved aside
    first and recorded, making replacement auditable and undoable.

SEE ALSO
    fpaste(1), fcopy(1), fcut(1), fdelete(1), fundo(1), fredo(1)

FREPLACE(1)                 User Commands                 FREPLACE(1)
