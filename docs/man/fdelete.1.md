FDELETE(1)                  User Commands                  FDELETE(1)

NAME
    fdelete - move files to temporary recovery storage and record the action

SYNOPSIS
    fdelete FILE...

DESCRIPTION
    fdelete moves one or more existing files or directories to the operating
    system's temporary location and appends a delete operation record to the
    fileclip history file.

    The history file is a JSON Lines file named history.jsonl in the platform
    fileclip state directory. The history has a hardcoded maximum of 1000
    records. When commands append records beyond this cap, the oldest records
    are deleted so that history.jsonl keeps at most 1000 records.

    fdelete is intended to be non-destructive. It does not permanently remove
    selected items. Instead, it recursively moves files and directories to /tmp
    on Unix or the platform's equivalent temporary directory so that they can
    later be recovered with fundo, fredo, or fpaste while the temporary copy
    still exists. The operating system or cleanup tools may later delete files
    from the temporary location.

OPTIONS
    fdelete does not accept options.

ARGUMENTS
    FILE...
        One or more files, directories, or other filesystem entries to move to
        temporary recovery storage.

        Shell-expanded patterns such as name*.txt are supported only because
        the user's shell expands them before fdelete starts. fdelete does not
        implement globbing or pattern matching itself.

BEHAVIOR
    If called with no arguments, fdelete should print:

        fdelete: nothing to delete

    to standard error and exit with status 1.

    fdelete validates every argument before moving anything. If any path is
    missing, cannot be resolved, cannot have metadata read, or is rejected as a
    dangerous target, no filesystem changes should be made and history should
    not be updated.

    Dangerous root or system-wide destructive targets must be rejected. Examples
    include /, filesystem roots, drive roots, and similarly broad targets. When
    possible, diagnostics for these rejected destructive actions should contain
    exactly:

        use rm instead

    On success, fdelete moves each selected item to a unique path below the
    operating system temporary directory, appends one history record with
    operation "delete", syncs history.jsonl, and prints one confirmation message
    per moved item.

    Directories are moved recursively. Symlinks are moved as symlinks rather
    than being followed for their final component.

HISTORY FORMAT
    fdelete records must use RFC3339/ISO-8601 UTC timestamps in
    YYYY-MM-DDTHH:MM:SSZ format for created_at. The created_at field is
    required; epoch-like strings such as 1781233086Z are not valid.

    fdelete records must keep the temporary recovery path in path fields and
    the original location in original_path fields. file_type should be one of
    file, dir, symlink, or other, aligned with fcopy and fpaste records.

    Intended fdelete records use this shape:

        {
          "id": "1781082377-25753875-335232",
          "operation": "delete",
          "created_at": "2026-06-10T09:06:17Z",
          "source_host": "laptop",
          "source_user": "fredy",
          "items": [
            {
              "path": "/tmp/fileclip-1781082377/file1.txt",
              "path_encoding": "unix-bytes-base64",
              "path_data": "L3RtcC9maWxlY2xpcC0xNzgxMDgyMzc3L2ZpbGUxLnR4dA==",
              "original_path": "/home/user/file1.txt",
              "original_path_encoding": "unix-bytes-base64",
              "original_path_data": "L2hvbWUvdXNlci9maWxlMS50eHQ=",
              "file_type": "file"
            }
          ]
        }

    The path fields identify the temporary recovery path. The original_path
    fields identify where fundo should restore the item. Consumers should prefer
    path_encoding/path_data and original_path_encoding/original_path_data over
    display paths when exact native path reconstruction is needed.

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
    Example success message:

        deleted to temp: /home/user/file1.txt -> /tmp/fileclip-1781082377/file1.txt

DIAGNOSTICS
    fdelete: nothing to delete
        No arguments were provided.

    fdelete: not found: ITEM
        The given path does not exist.

    fdelete: refusing to delete PATH: use rm instead
        The target is a filesystem root or similarly dangerous broad target.

    fdelete: cannot move SOURCE to DESTINATION: ERROR
        Moving the item to temporary recovery storage failed.

    fdelete: cannot write history PATH: ERROR
        The delete record could not be appended or synced.

EXIT STATUS
    0
        The command completed successfully and appended a history record.

    1
        The command failed. Failure cases include invalid arguments, rejected
        dangerous targets, move failures, and history write failures.

EXAMPLES
    Delete one file recoverably:

        fdelete notes.txt

    Delete several shell-expanded files:

        fdelete image*.png

    Recover the latest delete action:

        fundo

LIMITATIONS
    fdelete relies on the operating system temporary directory for recovery.
    Temporary files may be removed by the system or cleanup tools before fundo,
    fredo, or fpaste can recover them.

    fdelete does not implement globbing. Patterns are expanded by the shell.

DESIGN NOTES
    fdelete is intentionally recoverable and therefore does not prompt by
    default. Permanent deletion should be performed with rm or an equivalent
    system command, not fdelete.

SEE ALSO
    fcopy(1), fcut(1), fpaste(1), fundo(1), fredo(1), freplace(1)

FDELETE(1)                  User Commands                  FDELETE(1)
