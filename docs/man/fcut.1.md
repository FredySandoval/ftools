FCUT(1)                     User Commands                     FCUT(1)

NAME
    fcut - record file paths as cut sources in the fileclip history

SYNOPSIS
    fcut FILE...

DESCRIPTION
    fcut is the intended fileclip producer for cut-style operations. It will
    validate one or more existing files or directories, resolve each argument
    to an absolute path, and append a cut operation record to the fileclip
    history file.

    The history file is a JSON Lines file named history.jsonl. Each successful
    invocation should insert exactly one JSON object record describing the cut
    sources selected by the user.

    fcut records paths only. It does not delete, truncate, or otherwise modify
    source items immediately. Source items remain in place after fcut records
    them.

    The history has a hardcoded maximum of 1000 records. After inserting a new
    record, if the history exceeds this cap, fcut should delete the oldest
    records so that history.jsonl respects the 1000-record hardcoded limit.

OPTIONS
    fcut does not accept options.

ARGUMENTS
    FILE...
        One or more files, directories, or other filesystem entries to record
        in the fileclip history as cut sources.

        Each argument should be resolved to an absolute path with the platform
        path resolution behavior while preserving the selected filesystem entry.
        If the final path component is a symbolic link, fcut records the
        symlink path itself rather than the symlink target.

        If any path is missing, cannot be resolved, or its metadata cannot be
        read, fcut should print an error message to standard error and should
        not update the history file.

BEHAVIOR
    If called with no arguments, fcut should print:

        fcut: nothing to cut

    to standard error and exit with status 1.

    If called with one or more arguments, fcut should validate every argument
    before writing history. The history should be updated only if every
    argument is valid. There should be no partial history update when any input
    path is invalid.

    On success, fcut should create the fileclip state directory if needed, open
    or create history.jsonl, append one JSON object line with operation "cut",
    sync the file, and then print one confirmation message per recorded path.

    Existing history contents should be preserved until the hardcoded
    1000-record cap is reached. New successful invocations insert new records.
    If inserting a record would exceed the cap, fcut should remove the oldest
    record or records and keep at most 1000 records in history.jsonl.


HISTORY FORMAT
    The history file is a JSON Lines file. Each line is one JSON object.

    Intended fcut records use this shape:

        {
          "id": "1781082377-25753875-335232",
          "operation": "cut",
          "created_at": "2026-06-10T09:06:17Z",
          "source_host": "laptop",
          "source_user": "fredy",
          "items": [
            {
              "path": "/home/user/file1.txt",
              "path_encoding":"unix-bytes-base64",
              "path_data": "L2hvbWUvdXNlci9maWxlMS50eHQ=",
              "file_type": "file"
            },
            {
              "path": "/home/user/link-to-other",
              "path_encoding":"unix-bytes-base64",
              "path_data": "L2hvbWUvdXNlci9saW5rLXRvLW90aGVy",
              "file_type": "symlink"
            }
          ]
        }

    In the actual history.jsonl file, each object should be written on a
    single line:

        {"id":"1781082377-25753875-335232","operation":"cut","created_at":"2026-06-10T09:06:17Z","source_host":"laptop","source_user":"fredy","items":[{"path":"/home/user/file1.txt","path_encoding":"unix-bytes-base64","path_data":"L2hvbWUvdXNlci9maWxlMS50eHQ=","file_type":"file"},{"path":"/home/user/link-to-other","path_encoding":"unix-bytes-base64","path_data":"L2hvbWUvdXNlci9saW5rLXRvLW90aGVy","file_type":"symlink"}]}

    Fields:

    id
        A generated string identifier based on seconds since the Unix epoch,
        subsecond nanoseconds, and process id, separated by hyphens.

    operation
        The operation name. fcut should write cut.

    created_at
        The creation time as a UTC timestamp in YYYY-MM-DDTHH:MM:SSZ format.

    source_host
        The source host name. This is best-effort and may be unknown if the
        host name cannot be determined from the environment or system files.

    source_user
        The source user name. This is best-effort and may be unknown if the
        user name cannot be determined from the environment.

    items
        An array of recorded filesystem entries. Each item contains:

        path
            The resolved absolute path in a human-readable display form. This
            field is convenient for inspection, but consumers that need exact
            reconstruction should use path_encoding and path_data.

        path_encoding
            Identifies how path_data was produced. Current values are
            unix-bytes-base64 for Unix native path bytes,
            windows-utf16le-base64 for Windows UTF-16 code units serialized as
            little-endian bytes, and utf8-base64 for fallback platforms that
            use the UTF-8/display representation.

        path_data
            Base64-encoded native path representation. On Unix this preserves
            exact path bytes, including non-UTF-8 paths. On Windows this
            preserves the exact UTF-16 code units from the native path.

        file_type
            One of file, dir, symlink, or other, based on filesystem metadata at
            record time. Symbolic links are classified as symlink without
            following the final link target. The actual source metadata may
            change after fcut records it.

FILES
    Linux and other Unix systems:
        $XDG_STATE_HOME/fileclip/history.jsonl
        ~/.local/state/fileclip/history.jsonl

        If XDG_STATE_HOME is set and not empty, fcut should use it. Otherwise
        it should fall back to ~/.local/state/fileclip/history.jsonl.

    macOS:
        ~/Library/Application Support/fileclip/history.jsonl

    Windows:
        %LOCALAPPDATA%\fileclip\history.jsonl
        %USERPROFILE%\AppData\Local\fileclip\history.jsonl

        If LOCALAPPDATA is set and not empty, fcut should use it. Otherwise it
        should fall back to USERPROFILE\AppData\Local\fileclip\history.jsonl.

OUTPUT
    For each recorded item, fcut should print a confirmation message to
    standard output after the history file has been successfully updated:

        cut to clipboard: /absolute/path

    The wording says clipboard for compatibility with the current user-facing
    interface, even though the persistent source of truth is history.jsonl.

DIAGNOSTICS
    fcut: nothing to cut
        No arguments were provided.

    fcut: not found: ITEM
        The given path does not exist.

    fcut: cannot resolve ITEM: ERROR
        Resolving the path to an absolute path failed for a reason other than
        the path being missing.

    fcut: cannot read metadata for PATH: ERROR
        fcut resolved the path, but could not read filesystem metadata.

    fcut: cannot determine history directory: ERROR
        The required environment variables for the current platform were not
        available.

    fcut: cannot create history directory PATH: ERROR
        The fileclip state directory could not be created.

    fcut: cannot open history PATH: ERROR
        history.jsonl could not be opened or created.

    fcut: cannot lock history PATH: ERROR
        history.jsonl could not be locked before appending.

    fcut: cannot write history PATH: ERROR
        The history record could not be appended or synced.

    fcut: cannot unlock history PATH: ERROR
        history.jsonl was written and synced, but releasing the explicit lock
        reported an error.

EXIT STATUS
    0
        The command completed successfully and appended a cut history record.
        Source items were not deleted or otherwise modified by fcut itself.

    1
        The command failed. The history file should not be intentionally
        updated when validation fails before writing.

        Failure cases include:

        - No arguments were provided.
        - Any path is missing.
        - Any path cannot be resolved.
        - Metadata for any resolved path cannot be read.
        - The history directory cannot be determined.
        - The history directory cannot be created.
        - history.jsonl cannot be opened or created.
        - The history record cannot be written or synced.

EXAMPLES
    Record one file path as a future cut source:

        fcut notes.txt

    Record multiple paths as future cut sources:

        fcut file1.txt dir1 image.png

    Record paths expanded by the shell:

        fcut image*.png

    fcut does not implement pattern matching itself. Patterns such as
    image*.png are expanded by the user's shell before fcut receives the
    arguments.

    Example history.jsonl contents after two successful invocations:

        {"id":"1781082377-25753875-335232","operation":"cut","created_at":"2026-06-10T09:06:17Z","source_host":"laptop","source_user":"fredy","items":[{"path":"/home/user/a.txt","path_encoding":"unix-bytes-base64","path_data":"L2hvbWUvdXNlci9hLnR4dA==","file_type":"file"}]}
        {"id":"1781082380-123456789-335233","operation":"cut","created_at":"2026-06-10T09:06:20Z","source_host":"laptop","source_user":"fredy","items":[{"path":"/home/user/b.txt","path_encoding":"unix-bytes-base64","path_data":"L2hvbWUvdXNlci9iLnR4dA==","file_type":"file"},{"path":"/home/user/c.txt","path_encoding":"unix-bytes-base64","path_data":"L2hvbWUvdXNlci9jLnR4dA==","file_type":"file"}]}

LIMITATIONS
    fcut is not implemented yet. This page describes the intended behavior for
    a future command.

    fcut stores paths only. It does not delete or reserve source items by
    itself. Source items remain at their original paths after fcut records
    them and may later be changed, renamed, removed, or become inaccessible.

    fcut should enforce a hardcoded maximum history size of 1000 records. This
    limit is not currently configurable. When a new record is inserted and the
    cap is reached or exceeded, fcut should delete the oldest record or records
    to keep the history within the 1000-record limit.

    fcut should use platform-specific path resolution behavior while preserving
    the final selected path component when it is a symbolic link.

    The path field is a display representation for readability. Exact native
    path reconstruction should use path_encoding and path_data.

    fcut should use an explicit file lock while appending and syncing
    history.jsonl. Lock semantics are platform-dependent and may be advisory on
    platforms whose filesystem locking APIs are advisory.

DESIGN NOTES
    history.jsonl is the single source of truth for fileclip history behavior.

    fcut should write operation "cut" so the history records the user's
    clipboard-style intent.

    fcut stores fileclip state in the user-local application state or data
    directory instead of writing directly to the user's home directory. This
    avoids cluttering the home directory, keeps fileclip data grouped under a
    dedicated application folder, and follows the conventions of each supported
    operating system.

    Recording paths without changing sources immediately matches common cut
    behavior: the cut command selects the source items and stores that
    selection in history.

FCUT(1)                     User Commands                     FCUT(1)
