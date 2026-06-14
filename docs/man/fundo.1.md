FUNDO(1)                    User Commands                    FUNDO(1)

NAME
    fundo - undo the latest undoable fileclip action

SYNOPSIS
    fundo

DESCRIPTION
    fundo reads history.jsonl from the fileclip state directory, finds the
    latest undoable action, and attempts to reverse it back to its original
    state.

    The history file is a JSON Lines file named history.jsonl. The history has
    a hardcoded maximum of 1000 records. Commands that append records delete
    the oldest records when needed so that history.jsonl keeps at most 1000
    records.

    Undoable actions include recoverable deletes, replacements, pastes, and
    other history records that contain enough source and destination path data
    to reverse the filesystem operation.

OPTIONS
    fundo does not accept options.

ARGUMENTS
    fundo does not accept arguments.

BEHAVIOR
    fundo checks the latest currently undoable history action and undoes that
    action. It does not ask for confirmation because fileclip actions are
    intended to be recoverable.

    fundo uses undo-stack semantics. It must not select an action that has
    already been successfully undone by a later undo record, unless that undo
    has later been redone. A repeated fundo should undo the next earlier
    currently undoable action, or report no undoable history records. Undo
    records must identify source_record_id and source_operation so this stack
    behavior is possible.

    For a delete record, fundo moves each item from its temporary recovery path
    back to its recorded original path. For a paste result record, fundo
    reverses the recorded destination changes. For a replace result record,
    fundo must first remove or move away the replacement destination at
    destination_path, then restore displaced_path to original_destination_path
    when displaced_path exists. History should represent both phases enough for
    audit; if only restoration is represented in undo items, replacement
    removal is still part of undo semantics and should be recorded with
    status/details where practical.

    If some files for the selected action are no longer present, fundo should
    inform the user and continue attempting the remaining paths for that same
    action. Missing temporary recovery files, missing pasted destinations, or
    already-restored items should not prevent independent items in the same
    action from being attempted.

    After attempting an undo, fundo appends an undo record describing the
    source action, successful items, and failed items. That undo record lets
    fredo redo what fundo undid.

HISTORY FORMAT
    fundo reads the same JSON Lines history file used by fileclip commands.
    Each line is one JSON object. Records should include native path encoding
    fields such as path_encoding and path_data when exact reconstruction is
    needed.

    Undo records must include id, operation="undo", created_at,
    source_record_id, source_operation, failed_count, items, and failed_items.
    created_at is required and must be an RFC3339/ISO-8601 UTC timestamp in
    YYYY-MM-DDTHH:MM:SSZ format; epoch-like strings such as 1781233086Z are
    not valid. Successful items must include from_path and to_path with native
    path encoding/data fields when available, file_type if known, and
    status="undone". Failed items must include path or from_path/to_path as
    applicable, status="failed", and error.

    Intended undo records use this shape:

        {
          "id": "1781082500-111111111-335250",
          "operation": "undo",
          "created_at": "2026-06-10T09:08:20Z",
          "source_record_id": "1781082377-25753875-335232",
          "source_operation": "delete",
          "failed_count": 1,
          "items": [
            {
              "from_path": "/tmp/fileclip-1781082377/file1.txt",
              "to_path": "/home/user/file1.txt",
              "file_type": "file",
              "status": "undone"
            }
          ],
          "failed_items": [
            {
              "from_path": "/tmp/fileclip-1781082377/missing.txt",
              "to_path": "/home/user/missing.txt",
              "status": "failed",
              "error": "not found"
            }
          ]
        }

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
    fundo prints one per-item success message for each successfully undone
    item. The required form is:

        undone: /tmp/fileclip-1781082377/file1.txt -> /home/user/file1.txt

    Missing paths must print a per-item diagnostic and fundo must continue
    attempting remaining independent items. The required form is:

        fundo: not found: /tmp/fileclip-1781082377/missing.txt

DIAGNOSTICS
    fundo: history not found: PATH
        The fileclip history file does not exist.

    fundo: no undoable history records
        No record with enough information to undo was found.

    fundo: not found: PATH
        A path needed to undo an item no longer exists. This per-item
        diagnostic is required for missing paths; a final summary such as
        fundo: one or more items failed may also be printed, but it is not a
        substitute for per-item diagnostics.

    fundo: cannot undo SOURCE to DESTINATION: ERROR
        Moving, copying, deleting, or restoring an item failed.

    fundo: cannot write history PATH: ERROR
        The undo record could not be appended or synced.

EXIT STATUS
    0
        The selected action was undone successfully for every item.

    1
        The command failed, or one or more items in the selected action could
        not be undone.

EXAMPLES
    Recover files most recently moved by fdelete:

        fdelete notes.txt old-dir
        fundo

    Undo a replacement:

        freplace
        fundo

LIMITATIONS
    fundo depends on history.jsonl and on recovery paths still existing. Items
    moved to the operating system temporary directory may be removed by system
    cleanup before fundo runs.

    If history has exceeded the 1000-record cap, older undo sources may have
    been removed from history and cannot be discovered by fundo.

DESIGN NOTES
    fundo operates on one selected action at a time. Item-level failures are
    reported, but independent remaining items from the same action should still
    be attempted.

SEE ALSO
    fdelete(1), fredo(1), fpaste(1), freplace(1)

FUNDO(1)                    User Commands                    FUNDO(1)
