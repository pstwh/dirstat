# dirstat

See what takes up space on your disk, then clean it up. A fast WinDirStat for Linux, Windows and macOS.

![dirstat](docs/screenshot.png)

## Download

Get the latest build from [Releases](../../releases):

- **Linux**: `dirstat-*-x86_64-linux.tar.gz`
- **Windows**: `dirstat-*-x86_64-windows.zip`
- **macOS (Apple Silicon)**: `dirstat-*-aarch64-macos.zip`. The app is not notarized, so after unzipping run `xattr -dr com.apple.quarantine dirstat.app` once

## Use

Pick a drive or a folder. Click a tile to find it in the list, double-click to zoom in, right-click for actions. `Ctrl+K` opens every command, `F1` lists the shortcuts.

```bash
dirstat ~/Downloads        # open and scan a folder
dirstat scan / --top 20    # print the biggest items without a window
```

## Build

```bash
make run
```

Linux needs `libgtk-3-dev libxkbcommon-dev libwayland-dev`.
