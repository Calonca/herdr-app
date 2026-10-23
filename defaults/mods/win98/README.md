# Herdr 98

A Windows 98 layout for Herdr: `layouts/win98.json` + `themes/win98.json` +
this mod. One mod, many roles (`main.js`): taskbar, desktop, window (pane
title bars), menu (Start menu too), dialog, mines, sol, clippy. `kit.js`
draws the Win98 bits (bevels, buttons, title bars) in whole device pixels at
any zoom; Display Properties > Settings switches Small/Large Fonts (1x/2x).

Fonts: `fonts/` must be installed for the user (Linux:
`~/.local/share/fonts/herdr98/`, then `fc-cache`; macOS: Font Book), then
restart Herdr. Pixelated MS Sans Serif (from 98.css) for the UI, PxPlus IBM
VGA 8x16 (int10h.org, CC BY-SA 4.0) for terminals.

Keys (`keys.json`, section `layout:win98`): `win98.start` (Ctrl+Esc),
`win98.taskman` (Ctrl+Shift+Esc), `win98.new_tab`, `win98.new_space`,
`win98.rename_tab`, `win98.rename_space`; also `run`, `shutdown`, `about`,
`display`, `welcome`, `boot`, `mines`, `sol`, `mycomputer`, `network`,
`bin`, `datetime`, `logoff`, `small_fonts`, `large_fonts`.

`art/` holds the generators the icons, images and sounds came from (paths
inside point at the build folder they ran in).
