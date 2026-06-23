
<img width="798" height="184" alt="Frame 10 (1)" src="https://github.com/user-attachments/assets/35f08e35-6847-4919-9cff-1442bad9308e" />


Command-line file clipboard tools for copying, cutting, pasting, deleting, replacing, undoing, and redoing file operations with a JSON Lines history.

## Commands

| Command | Description |
| --- | --- |
| `fcopy` | Record existing file or directory paths for a later paste. |
| `fcut` | Record existing file or directory paths as cut/move sources for a later paste. |
| `fpaste` | Paste the latest supported history source into the current directory; refuses destination collisions and suggests `freplace`. |
| `fdelete` | Move files or directories to temporary recovery storage and record the action. |
| `freplace` | Paste into the current directory and recoverably replace destination collisions. |
| `fundo` | Undo the latest undoable fileclip action. |
| `fredo` | Redo an undone fileclip action or repeat the latest repeatable action. |

## Install / build

Requires Rust edition 2024 support.

```sh
cargo build --release
```

The binaries are built in `target/release/`.

To install locally with Cargo:

```sh
cargo install --path .
```

## Quick example

```sh
fcopy ./notes.txt      # <- Copies one file to private clipboard
cd /tmp                # <- Change your directory
fpaste                 # <- you can paste ./notes.txt in the current directory

fcut ./old-name.txt    # <- will remove this file until paste
cd ../archive
fpaste

fdelete ./unwanted.txt # <- move file to /tmp
fundo                  # <- reverts last change
```

## History

Operations are stored in a capped JSON Lines history file named `history.jsonl` with up to 1000 records.

Default location:

- Linux/Unix: `$XDG_STATE_HOME/fileclip/history.jsonl` or `~/.local/state/fileclip/history.jsonl`
- macOS: `~/Library/Application Support/fileclip/history.jsonl`
- Windows: `%LOCALAPPDATA%\fileclip\history.jsonl` or `%USERPROFILE%\AppData\Local\fileclip\history.jsonl`

## More details

See `docs/man/*.1.md` for detailed command documentation.
