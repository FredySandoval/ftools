FPASTE(1)                   User Commands                   FPASTE(1)

NAME
    fpaste - paste the latest supported fileclip history items into the current directory

SYNOPSIS
    fpaste

DESCRIPTION
    fpaste reads the fileclip history file and applies the most recent
    supported clipboard-style operation to the current working directory.

    The history file is a JSON Lines file named history.jsonl. Each line is one
    JSON object record. The history has a hardcoded maximum of 1000 records.
    Commands that append history records delete the oldest records when needed
    so that history.jsonl respects the 1000-record hardcoded limit.

    fcopy currently writes copy operation records to this file. A supported
    copy record causes fpaste to copy each recorded source item into the current
    working directory. Supported move and cut records cause fpaste to use the
    recorded paths as paste sources and move each still-existing source item
    into the current working directory. A supported delete record points to
    items that were moved to the operating system's temporary location,
    allowing fpaste to recover those items if they still exist there.

    fpaste consumes paths recorded in history. When an item contains
    path_encoding and path_data, fpaste should use those fields to reconstruct
    the native source path as exactly as possible. The human-readable path field
    is a fallback display representation and is less exact on platforms where
    native paths are not always UTF-8.

    fpaste operates on file contents and directory trees. It reads paths from
    producer commands such as fcopy, fcut, or fdelete, performs filesystem copy
    or move operations from those recorded paths, and after destination
    preflight succeeds and item processing is attempted, appends its own paste
    result record to history.jsonl.

OPTIONS
    fpaste does not accept options.

ARGUMENTS
    fpaste does not accept arguments.

BEHAVIOR
    fpaste opens the fileclip history file, reads records from newest to
    oldest, ignores unsupported records, and selects the most recent supported
    path-source operation record. This lets fpaste skip past its own paste
    records and any other unsupported history entries until it finds the latest
    usable copy, move, cut, or delete record.

    After destination preflight succeeds and fpaste attempts the paste, fpaste
    appends a new paste record to history.jsonl describing the completed paste
    result, including item-level failures such as missing sources. The history
    remains capped at 1000 records; if appending the paste record exceeds the
    cap, the oldest record or records are removed.

    Supported operations are:

    copy
        Copy each source item from the selected record into the current working
        directory.

    move
        Move each source item from the selected record into the current working
        directory.

    cut
        Move each source item from the selected record into the current working
        directory. A cut record is paste-compatible with move behavior: the
        history stores the source paths, and fpaste uses those paths when the
        user later decides where to paste them.

    delete
        Recover each still-existing item from the operating system's temporary
        location into the current working directory. A delete record does not
        represent immediate permanent removal. Instead, a delete-style producer
        should move the item to /tmp or the platform's equivalent temporary
        location and store that temporary path in history. fpaste can then use
        the delete record to restore the item if the user changes their mind
        before the system or another cleanup process removes it.

    Unsupported, malformed, or unreadable records are not valid paste sources.
    If the newest history record is unsupported, including a prior fpaste paste
    result record, fpaste should continue looking backward for the most recent
    supported path-source record.

    For each item in the selected record, fpaste reconstructs the source path,
    determines the destination as a child of the current working directory with
    the same final path component, and then attempts the requested operation.

    Before copying or moving anything, fpaste checks the destination path for
    every item in the selected record. If any destination already exists,
    fpaste reports the collision, marks the colliding item as failed, aborts
    the paste, and makes no filesystem changes for any item in the record.

    Missing sources are reported. After destination preflight succeeds, fpaste
    continues processing remaining items when possible so that one missing or
    failed item does not prevent independent later items from being considered.
    If one or more items fail because their source paths do not exist or cannot
    be processed, fpaste appends a paste result record that includes successful
    items, if any, plus the failed count and the paths of the failed items.

    For copy records, files and other copyable filesystem entries are copied to
    the destination path. Directories are copied recursively so that their
    contents are reproduced below the destination directory. Symlinks are
    recreated as symlinks at the destination instead of being dereferenced;
    broken symlinks can be pasted as symlink entries.

    For move and cut records, items are moved from their recorded source path
    to the destination path when the source still exists. Symlinks remain
    symlinks, including when a move has to fall back to copy followed by
    removal across a filesystem boundary. For delete records,
    items are moved from their recorded temporary recovery path to the
    destination path when that temporary source still exists. A move may be
    implemented as a rename when possible, or as copy followed by removal when
    needed by the platform or filesystem boundary.

    If a destination path already exists, fpaste does not overwrite it, merge
    into it, or choose a new name for it. Destination collisions are checked
    before any copy or move is attempted. If any destination collision is found,
    fpaste fails the whole paste operation and leaves all source and destination
    paths unchanged.

HISTORY FORMAT
    fpaste reads and writes the same JSON Lines history file used by fileclip
    commands. Each line is one JSON object. The history file is capped at 1000
    records; when commands insert new records beyond that cap, the oldest
    records are removed.

    Current fcopy records use this shape:

        {
          "id": "1781082377-25753875-335232",
          "operation": "copy",
          "created_at": "2026-06-10T09:06:17Z",
          "source_host": "laptop",
          "source_user": "fredy",
          "items": [
            {
              "path": "/home/user/file1.txt",
              "path_encoding": "unix-bytes-base64",
              "path_data": "L2hvbWUvdXNlci9maWxlMS50eHQ=",
              "file_type": "file"
            },
            {
              "path": "/home/user/other",
              "path_encoding": "unix-bytes-base64",
              "path_data": "L2hvbWUvdXNlci9vdGhlcg==",
              "file_type": "dir"
            },
            {
              "path": "/home/user/link",
              "path_encoding": "unix-bytes-base64",
              "path_data": "L2hvbWUvdXNlci9saW5r",
              "file_type": "symlink"
            }
          ]
        }

    In history.jsonl, each object is written on a single line:

        {"id":"1781082377-25753875-335232","operation":"copy","created_at":"2026-06-10T09:06:17Z","source_host":"laptop","source_user":"fredy","items":[{"path":"/home/user/file1.txt","path_encoding":"unix-bytes-base64","path_data":"L2hvbWUvdXNlci9maWxlMS50eHQ=","file_type":"file"},{"path":"/home/user/other","path_encoding":"unix-bytes-base64","path_data":"L2hvbWUvdXNlci9vdGhlcg==","file_type":"dir"}]}

    After destination preflight succeeds and fpaste attempts the paste, fpaste
    appends a paste result record. Paste result records are ignored by future
    fpaste searches for source paths, but they document what was pasted, where
    it was placed, where it was pasted from, and which items failed. A paste
    result record may use this shape:

        {
          "id": "1781082400-987654321-335240",
          "operation": "paste",
          "created_at": "2026-06-10T09:06:40Z",
          "source_host": "laptop",
          "source_user": "fredy",
          "destination_host": "workstation",
          "destination_user": "fredy",
          "source_record_id": "1781082377-25753875-335232",
          "source_operation": "copy",
          "destination_dir": "/tmp/destination",
          "failed_count": 1,
          "items": [
            {
              "source_path": "/home/user/file1.txt",
              "source_path_encoding": "unix-bytes-base64",
              "source_path_data": "L2hvbWUvdXNlci9maWxlMS50eHQ=",
              "destination_path": "/tmp/destination/file1.txt",
              "destination_path_encoding": "unix-bytes-base64",
              "destination_path_data": "L3RtcC9kZXN0aW5hdGlvbi9maWxlMS50eHQ=",
              "file_type": "file",
              "status": "pasted"
            },
            {
              "source_path": "/home/user/other",
              "source_path_encoding": "unix-bytes-base64",
              "source_path_data": "L2hvbWUvdXNlci9vdGhlcg==",
              "destination_path": "/tmp/destination/other",
              "destination_path_encoding": "unix-bytes-base64",
              "destination_path_data": "L3RtcC9kZXN0aW5hdGlvbi9vdGhlcg==",
              "file_type": "dir",
              "status": "pasted"
            }
          ],
          "failed_items": [
            {
              "source_path": "/home/user/missing.txt",
              "source_path_encoding": "unix-bytes-base64",
              "source_path_data": "L2hvbWUvdXNlci9taXNzaW5nLnR4dA==",
              "file_type": "file",
              "status": "failed",
              "error": "not found"
            }
          ]
        }

    In the actual history.jsonl file, the paste result object is also written
    on a single line:

        {"id":"1781082400-987654321-335240","operation":"paste","created_at":"2026-06-10T09:06:40Z","source_host":"laptop","source_user":"fredy","destination_host":"workstation","destination_user":"fredy","source_record_id":"1781082377-25753875-335232","source_operation":"copy","destination_dir":"/tmp/destination","failed_count":1,"items":[{"source_path":"/home/user/file1.txt","source_path_encoding":"unix-bytes-base64","source_path_data":"L2hvbWUvdXNlci9maWxlMS50eHQ=","destination_path":"/tmp/destination/file1.txt","destination_path_encoding":"unix-bytes-base64","destination_path_data":"L3RtcC9kZXN0aW5hdGlvbi9maWxlMS50eHQ=","file_type":"file","status":"pasted"},{"source_path":"/home/user/other","source_path_encoding":"unix-bytes-base64","source_path_data":"L2hvbWUvdXNlci9vdGhlcg==","destination_path":"/tmp/destination/other","destination_path_encoding":"unix-bytes-base64","destination_path_data":"L3RtcC9kZXN0aW5hdGlvbi9vdGhlcg==","file_type":"dir","status":"pasted"}],"failed_items":[{"source_path":"/home/user/missing.txt","source_path_encoding":"unix-bytes-base64","source_path_data":"L2hvbWUvdXNlci9taXNzaW5nLnR4dA==","file_type":"file","status":"failed","error":"not found"}]}

    Fields used by fpaste:

    operation
        The requested operation. fpaste uses copy, move, cut, and delete records
        as supported path-source records. fcopy currently writes copy records.
        After destination preflight succeeds and item processing is attempted,
        fpaste writes a paste result record. Paste result records are history
        events, not path-source records, so future fpaste invocations ignore
        them while searching backward for the latest supported path-source
        record.

    source_host
        For path-source records, the host where the source record was created.
        For paste result records, the host from the selected source record.

    source_user
        For path-source records, the user that created the source record. For
        paste result records, the user from the selected source record.

    destination_host
        For paste result records, the host where fpaste wrote the destination
        items. This is best-effort and may be unknown if the host name cannot
        be determined.

    destination_user
        For paste result records, the user that ran fpaste. This is best-effort
        and may be unknown if the user name cannot be determined.

    source_record_id
        For paste result records, the id of the history record that fpaste used
        as the path source.

    source_operation
        For paste result records, the operation from the selected path-source
        record, such as copy, move, cut, or delete.

    destination_dir
        For paste result records, the current working directory where fpaste
        attempted to place items.

    failed_count
        For paste result records, the number of items that failed. fpaste does
        not store a succeeded_count field; successful items are represented by
        the entries in items whose status is pasted.

    items
        For path-source records, an array of recorded filesystem entries. Each
        item contains path data for a source item. For paste result records,
        items contains only successful pasted items.

    failed_items
        For paste result records, an array of failed items. Each failed item
        stores the source path and native path encoding data when available,
        plus a status and error string. Failed items are recorded for item-level
        failures such as missing source paths after destination preflight has
        succeeded. Destination-collision failures abort before filesystem
        changes and do not append a paste result record.

    path
        The resolved absolute path in a human-readable display form. fpaste may
        use this as a fallback when exact native reconstruction is not possible.

    path_encoding
        Identifies how path_data was produced. Current values written by fcopy
        are unix-bytes-base64 for Unix native path bytes,
        windows-utf16le-base64 for Windows UTF-16 code units serialized as
        little-endian bytes, and utf8-base64 for fallback platforms that use the
        UTF-8/display representation.

    path_data
        Base64-encoded native path representation. fpaste should prefer this
        field together with path_encoding when reconstructing source paths.

    file_type
        One of file, dir, symlink, or other, based on source metadata at
        record time. The actual source metadata may have changed by the time
        fpaste runs. fpaste determines paste behavior from current filesystem
        metadata and preserves current symlinks as symlinks.

FILES
    Linux and other Unix systems:
        $XDG_STATE_HOME/fileclip/history.jsonl
        ~/.local/state/fileclip/history.jsonl

        If XDG_STATE_HOME is set and not empty, fpaste uses it. Otherwise it
        falls back to ~/.local/state/fileclip/history.jsonl.

    macOS:
        ~/Library/Application Support/fileclip/history.jsonl

    Windows:
        %LOCALAPPDATA%\fileclip\history.jsonl
        %USERPROFILE%\AppData\Local\fileclip\history.jsonl

        If LOCALAPPDATA is set and not empty, fpaste uses it. Otherwise it
        falls back to USERPROFILE\AppData\Local\fileclip\history.jsonl.

OUTPUT
    fpaste prints per-item messages to standard output for successful copy,
    move, cut, or delete-record paste operations. The messages should identify
    the source item and the destination in the current working directory.

    Example successful copy message:

        pasted: /home/user/file1.txt -> ./file1.txt

    Example successful move, cut, or delete-record paste message:

        moved: /home/user/file1.txt -> ./file1.txt

    After processing the selected record, fpaste prints a summary message that
    reports how many items succeeded and how many failed. If destination
    preflight succeeded and fpaste attempted item processing, fpaste then
    appends a paste result record to history.jsonl, even when some item-level
    failures occurred.

    Example summary:

        fpaste: 2 succeeded, 1 failed

DIAGNOSTICS
    fpaste: history not found: PATH
        The fileclip history file does not exist.

    fpaste: cannot determine history directory: ERROR
        The required environment variables for the current platform were not
        available.

    fpaste: cannot open history PATH: ERROR
        history.jsonl could not be opened for reading or appending.

    fpaste: no supported history records
        No readable history record with a supported path-source operation was
        found. Unsupported records, including prior fpaste paste result records,
        were ignored.

    fpaste: malformed history record: ERROR
        A history line could not be parsed as a supported JSON object.

    fpaste: cannot reconstruct path for item: ERROR
        The item did not contain usable path_encoding/path_data or a usable
        fallback path.

    fpaste: not found: PATH
        A recorded source path does not exist at paste time.

    fpaste: cannot copy SOURCE to DESTINATION: ERROR
        A copy operation failed.

    fpaste: cannot move SOURCE to DESTINATION: ERROR
        A move, cut, or delete-record paste operation failed.

    fpaste: cannot write history PATH: ERROR
        The paste result record could not be appended or synced.

    fpaste: error: same file name already exists: DESTINATION
    fpaste: paste stopped before changing files
    To overwrite, use freplace instead
        The destination path already exists with the same final file name that
        fpaste would create. fpaste does not overwrite, merge, or rename
        existing destinations. If any destination exists, fpaste aborts before
        making filesystem changes and marks the colliding item as failed.
        Because destination checking failed, no paste result record is appended.

EXIT STATUS
    0
        The command completed successfully and every item in the selected
        history record was pasted into the current working directory.

    1
        The command failed, or one or more items from the selected record could
        not be pasted.

        Failure cases include:

        - The history directory cannot be determined.
        - history.jsonl does not exist or cannot be opened.
        - No supported path-source history record can be found.
        - A source path cannot be reconstructed.
        - A recorded source path is missing.
        - A destination path already exists.
        - A destination path cannot be created.
        - A copy, move, cut, or delete-record paste operation fails.
        - The paste result record cannot be appended or synced.

EXAMPLES
    Copy the most recent fcopy record into the current directory:

        cd /tmp/destination
        fcopy ~/notes.txt ~/images
        cd /tmp/destination
        fpaste

    If the selected record contains /home/user/notes.txt and /home/user/images,
    fpaste creates:

        /tmp/destination/notes.txt
        /tmp/destination/images

    Move items from a future or companion command that records a move, cut, or
    delete operation:

        fpaste

    For a move or cut record, each still-existing source item is moved into the
    current working directory instead of copied. For a delete record, each
    still-existing temporary recovery item is moved back into the current
    working directory.

    Paste the latest supported record even if newer unsupported records exist:

        fpaste

    fpaste searches backward through history.jsonl, ignores unsupported records
    including prior fpaste paste result records, and uses the latest supported
    copy, move, cut, or delete path-source operation.

LIMITATIONS
    fcopy currently writes copy records. Move, cut, and delete records require
    a producer that writes operation "move", "cut", or "delete" records using
    the fileclip history format.

    fileclip history has a hardcoded maximum size of 1000 records. This limit
    is not currently configurable. When commands append new records and the cap
    is reached or exceeded, the oldest record or records are deleted to keep
    history.jsonl within the 1000-record limit. Very old paste sources may
    therefore disappear from history.

    fpaste depends on the recorded paths still being accessible. For copy,
    move, and cut records, those paths are the original source paths. For delete
    records, those paths should be temporary recovery paths such as /tmp or the
    platform's equivalent temporary location. If a source was removed by system
    cleanup, renamed, unmounted, or is no longer reachable, that item cannot be
    pasted.

    history.jsonl stores source paths, not file contents. fpaste reads file
    contents from the original source paths at paste time.

    Exact native path reconstruction depends on path_encoding and path_data.
    If those fields are missing or unsupported, fpaste may fall back to the path
    display field, which can be lossy for non-UTF-8 or otherwise unusual paths.

    fpaste never overwrites, merges into, or renames an existing destination.
    If any destination path already exists, the paste is aborted before any
    filesystem changes are made. Use freplace instead when overwrite behavior
    is desired.

    File metadata preservation, symbolic link handling, special file handling,
    permissions, ownership, timestamps, and cross-filesystem move details are
    implementation-dependent unless explicitly documented by the installed
    fpaste version.

DESIGN NOTES
    history.jsonl is the single source of truth for fileclip history behavior.

    fpaste reads existing history records and does not need command-line
    arguments because the selected operation and source items come from
    history.jsonl. fpaste also writes a new history event after destination
    preflight succeeds and paste processing is attempted so that paste activity
    is auditable without becoming the next path-source operation.

    fpaste should prefer path_encoding and path_data over the display path so
    that paths written by fcopy can be reconstructed as accurately as possible
    across supported platforms.

    The destination for each pasted item is the current working directory. This
    mirrors common clipboard paste behavior: copy or cut elsewhere, change to a
    destination directory, and paste there.

SEE ALSO
    fcopy(1), fcut(1), fdelete(1)

FPASTE(1)                   User Commands                   FPASTE(1)
