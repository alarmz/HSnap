# HyperSnap 6 – functional specification (for clean-room rewrite)

Sources: the product's own manual (`HprSnap6Man.chm`, decompiled to `analysis/manual/`),
the install folder listing, and strings/imports Ghidra could see in `HprSnap6.exe`.

## Analysis findings

- `HprSnap6.exe` is a 32-bit (x86) PE, v6.30.2, MFC 8 + LEAD Tools imaging DLLs
  (`lf*10n.dll`, `lt*10n.dll`), LibTiff, LibJasPer, zlib/unzip.
- The executable is **packed/protected**: Ghidra (`analysis/out/`) recovers a single entry
  stub, and the import table contains only loader/anti-debug APIs
  (`DebugActiveProcess`, `WriteProcessMemory`, ...). I did **not** unpack it; this would
  defeat the vendor's protection and is not needed because the behaviour is documented.
- Supporting binaries: `HSTxtCap(64).dll` (TextSnap), `dxsnap.dll` (DirectX/Glide capture),
  `OverlayHook.dll`, `HsSizer6.dll`, `HprRes6.dll` (resources).
- No code, assets or resources from the original are copied. The Rust code is written
  from the behaviours below.

## Scope of the rewrite (core)

### Capture modes
| Mode | Behaviour |
|---|---|
| Full screen | Monitor containing the mouse pointer, edge to edge |
| Virtual desktop | All monitors in a single image |
| Active window | Foreground window (frame included) |
| Window or control | Hover highlights window/control under cursor; click captures it |
| Region | Rubber-band rectangle over frozen screen, magnifier + help text, Esc cancels |
| Repeat last capture | Re-run last mode with same parameters |
| Mouse cursor only | Cursor drawn on background colour |

Out of scope for v1: free-hand/multi-region, scrolling capture, DirectX/Glide, TextSnap OCR,
voice commands, FTP upload, printing.

### Capture settings
- Delay before capture (ms)
- Include mouse cursor (bool)
- Default background colour (+ transparent flag, honoured by PNG)
- Play sound (bool)
- Hide own window before capture / restore after
- Auto-repeat capture every N seconds; stop hotkey Shift+F11

### Hot keys (defaults taken from the manual)
- Ctrl+Shift+R region, Ctrl+Shift+S full screen, Ctrl+Shift+W window/control,
  Ctrl+Shift+M multi-region (not implemented), Ctrl+Shift+U text under cursor (not implemented)
- Shift+F11 stop auto capture
- Hot keys can be switched off globally.

### After-capture handling (View & Edit tab)
- New tab/document per capture (default); replace current; "do not change" (clipboard/save only)
- Optional auto-copy to clipboard, auto-save (QuickSave)

### QuickSave
- Root filename with trailing numeric group auto-incremented (`Img123-017.png` -> `Img123-018.png`)
- start / stop / step; loop back to start; zero padding derived from stop number
- printf-style `%04d` takes precedence

### Editing
- Tools: select, pencil, line, arrow, rectangle, ellipse, text, crop
- Undo / redo
- Image ops: crop, rotate/mirror (basic)
- Copy to clipboard, save as (PNG, JPG, BMP, GIF)
