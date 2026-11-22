# dirstat

See what takes up space on your disks, then clean it up. A fast, cross-platform take on WinDirStat for Linux, Windows and macOS, written in Rust with egui.

Pick a drive or a folder and the scan fills a cushion treemap, a sortable folder tree and a list of file types while it runs. Click a tile to find it in the tree, zoom into a folder, find the largest files or the duplicates, and move what you do not need to the trash.

## What it does

- **Scanning**: parallel and incremental, millions of files in seconds, with live totals, the current folder and a Stop button. Uses `getdents64` and `statx` on Linux and the directory listing data on Windows. Symlinks are never followed, hard links are counted once, other file systems are left out unless you ask, and unreadable folders are listed instead of failing the scan
- **Treemap**: squarified, with WinDirStat style cushion shading or flat tiles, colored by file type, rendered on all cores into one texture. Hover for path, size and date, click to select, double-click or `Enter` to zoom in, scroll the wheel to zoom in or out, right-click for actions. Breadcrumbs show where you are
- **Folders**: a tree of everything with a share bar, percent, size on disk and apparent size, items, files and modified date. Sort by any column, navigate with the keyboard, and see the hovered row outlined in the treemap
- **Types**: every extension with its color, total size, share and file count. Click one to light it up in the treemap, double-click to list its files
- **Largest**: the 1000 largest files of the current zoom
- **Duplicates**: same size, then same first and last 16 KB, then the same BLAKE3 hash of the whole file. Groups are sorted by the space they waste, with "keep newest" or "keep oldest" selection and one button to trash the rest
- **Search**: words plus filters, for example `invoice ext:pdf,docx size:>10mb older:1y kind:file`
- **Actions**: open, show in the file manager, copy path or name, rescan one folder, exclude a name from future scans, move to trash or delete permanently (with a confirmation that lists what goes and how much is freed)
- **Size on disk or apparent size**, switchable at any time
- **Export** to CSV or to the ncdu JSON format, and open ncdu JSON exports later (`ncdu -o` files work too)
- **Command line**: `dirstat scan PATH` prints the biggest items without opening a window
- **Settings**: theme (system, dark, light), cushion or flat treemap, brightness, cushion height, crossing file systems, hard link handling, thread count and exclusions

## Speed

Measured on Linux with a warm cache, 32 threads available:

| Folder | Items | dirstat | du -s |
|---|---|---|---|
| `/usr` | 631 K | 0.12 s | 1.08 s |
| `/` | 15.5 M | 2.8 s | |

Each item takes about 70 bytes: 48 for the node, 4 for the sort order and the name stored once in a shared buffer. The treemap of `/usr` takes 10 ms to lay out and 2.5 ms to shade at 1920x1080. Run `make bench DIR=/some/path` to measure your machine.

## Keys

`Ctrl` is `Cmd` on macOS.

| Anywhere | | Selection | |
|---|---|---|---|
| `Ctrl+K` | commands and quick find | `Enter` | zoom into it |
| `Ctrl+O` | scan a folder | `Backspace` | zoom out |
| `F5` | rescan everything | `Up` `Down` `Left` `Right` | move, collapse, expand |
| `Shift+F5` | rescan the selection | `Ctrl+Enter` | open |
| `Esc` | stop the scan, clear the highlight | `Ctrl+Shift+R` | show in file manager |
| `Ctrl+F` | search | `Ctrl+Shift+C` | copy the path |
| `Ctrl+D` | find duplicates | `Delete` | move to trash |
| `Ctrl+B` `Ctrl+M` | hide the lists or the treemap | `Shift+Delete` | delete permanently |
| `Ctrl+\` | side by side or stacked | `Ctrl+E` | export CSV |
| `Alt+1..5` | folders, types, largest, duplicates, search | `F1` | all shortcuts |

## Command line

```bash
dirstat                          # open the app
dirstat ~/Downloads              # open the app and scan a folder
dirstat --open scan.json         # open a saved scan
dirstat scan / --top 15 --depth 2
dirstat scan ~ --apparent --exclude node_modules --csv home.csv --json home.json
```

## Build

```bash
make run        # build and start
make test       # tests
make lint       # rustfmt and clippy
make install    # binary, desktop entry and icon into ~/.local
make windows    # x86_64 Windows build with cargo-zigbuild
```

Linux needs `libgtk-3-dev libxkbcommon-dev libwayland-dev` for the folder picker and the window. Settings live in `~/.config/dirstat`, `%APPDATA%\dirstat` or `~/Library/Application Support/dirstat`, or in `DIRSTAT_DATA_DIR`.

## Layout

- `crates/dirstat-core`: the tree (a flat arena of 48 byte nodes with `u32` ids, children in contiguous blocks), the parallel scanner (a rayon work-stealing walk feeding one collector thread that appends blocks and adds sizes up the parent chain), search, largest files, duplicates, export and import, disks and file operations
- `crates/dirstat-treemap`: squarified layout with pixel culling and the cushion rasterizer, independent of the UI
- `crates/dirstat-app`: the egui app and the command line
