FREDO(1)                    User Commands                    FREDO(1)

NAME
    fredo - redo an undone fileclip action or repeat the latest action

SYNOPSIS
    fredo

DESCRIPTION
    fredo reads the fileclip history file and chooses an action to perform
    again. fredo redoes the latest undo record that has not already been
    redone. If the latest action is not undo, fredo attempts to repeat that
    latest repeatable action best-effort.

    The history file is a JSON Lines file named history.jsonl in the platform
    fileclip state directory. The history has a hardcoded maximum of 1000
    records. Commands that append records delete the oldest records when needed
    so that history.jsonl keeps at most 1000 records.

OPTIONS
    fredo does not accept options.

ARGUMENTS
    fredo does not accept arguments.

BEHAVIOR
    If the newest redoable record is an undo record that has not already been
    redone, fredo uses the undo record's source action and item mapping to put
    the filesystem back into the state it had before fundo ran. For example, if
    fundo restored a file deleted by fdelete, fredo moves that file back to
    temporary recovery storage and records the redo.

    If the latest action is not an undo, fredo attempts to repeat that action
    best-effort. Repeating a copy-style record means applying the same
    operation to the current filesystem state. Repeating a delete, paste, or
    replace action uses the paths recorded in history as the intended operation
    inputs. Repeat result records must include source metadata so later history
    consumers can identify what was repeated.

    If a repeated or redone action fails, fredo should warn the user and record
    item-level failures when it reaches the point where history recording is
    appropriate. Failures can happen because paths have changed, temporary
    recovery files were cleaned up, destinations now exist, permissions changed,
    or the selected action is not repeatable.

HISTORY FORMAT
    fredo reads and writes the same JSON Lines history file used by fileclip
    commands. Each line is one JSON object.

    Redo records must include id, operation="redo", created_at,
    source_record_id, source_operation, failed_count, items, failed_items, and
    per-item statuses. created_at is required and must be an
    RFC3339/ISO-8601 UTC timestamp in YYYY-MM-DDTHH:MM:SSZ format; epoch-like
    strings such as 1781233086Z are not valid. When redo re-applies an undo,
    redone_operation must identify the operation being redone. Successful items
    must use status="redone" and failed items must use status="failed" with an
    error.

    Intended redo records use this shape:

        {
          "id": "1781082600-222222222-335260",
          "operation": "redo",
          "created_at": "2026-06-10T09:10:00Z",
          "source_record_id": "1781082500-111111111-335250",
          "source_operation": "undo",
          "redone_operation": "delete",
          "failed_count": 0,
          "items": [
            {
              "from_path": "/home/user/file1.txt",
              "to_path": "/tmp/fileclip-1781082600/file1.txt",
              "file_type": "file",
              "status": "redone"
            }
          ],
          "failed_items": []
        }

    Redo and repeat records are history events. Future commands may use them to
    understand what fredo attempted, but path-source commands should select only
    records whose operation and item fields are supported for their purpose.

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
    Example redo message:

        redone: /home/user/file1.txt -> /tmp/fileclip-1781082600/file1.txt

    Example warning:

        fredo: warning: could not repeat /home/user/file1.txt: not found

DIAGNOSTICS
    fredo: history not found: PATH
        The fileclip history file does not exist.

    fredo: no redoable history records
        No usable history record could be selected for redo or repeat.

    fredo: warning: could not repeat PATH: ERROR
        fredo attempted to repeat the latest non-undo action, but an item could
        not be processed.

    fredo: warning: could not redo PATH: ERROR
        fredo attempted to redo an undo action, but an item could not be
        processed.

    fredo: cannot write history PATH: ERROR
        The redo or repeat result record could not be appended or synced.

EXIT STATUS
    0
        The selected redo or repeat action completed successfully for every
        item.

    1
        The command failed, or one or more items could not be redone or
        repeated.

EXAMPLES
    Redo an undo:

        fdelete notes.txt
        fundo
        fredo

    Repeat the latest action when it is not an undo:

        fdelete old.log
        fredo

LIMITATIONS
    Repeating an action is best-effort. Paths, destinations, and temporary
    recovery locations may no longer match the state that existed when the
    original command ran.

    If history has exceeded the 1000-record cap, records needed to understand
    an older undo or repeat source may no longer be available.

DESIGN NOTES
    fredo intentionally has two modes: redo after undo, and repeat otherwise.
    This mirrors common editor behavior while still making non-undo fileclip
    actions convenient to attempt again.

SEE ALSO
    fundo(1), fdelete(1), fpaste(1), freplace(1)

FREDO(1)                    User Commands                    FREDO(1)
