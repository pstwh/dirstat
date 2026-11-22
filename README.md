# dirstat

See what takes up space on your disk, then clean it up. A fast WinDirStat for Linux, Windows and macOS.

![dirstat](docs/screenshot.png)

## Install

Linux (x86_64) and macOS (Apple Silicon):

```bash
curl -fsSL https://raw.githubusercontent.com/pstwh/dirstat/main/install.sh | sh
```

Windows (x64), in PowerShell:

```powershell
irm https://raw.githubusercontent.com/pstwh/dirstat/main/install.ps1 | iex
```

Run it again to update. Builds are also on the [Releases](../../releases) page.

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
