# HSnap vs. the HyperSnap 6 manual — feature matrix

Source of truth for "what the original does": `HprSnap6Man.chm` (its table of contents has 155 entries).
HSnap is a clean-room Rust rewrite; nothing was copied from the original program.

**Status**: ✅ done · 🟡 partial · ❌ not done · ➖ not applicable
**Verified** (how far I actually checked it):
- **T** — covered by an automated test (`cargo test`, 60 tests)
- **S** — run for real and looked at (screenshot, real window, real file, real printer driver)
- **C** — compiles and is wired in, but never exercised (no test, never clicked)

**Totals** (108 rows): ✅ 74 · 🟡 14 · ❌ 18 · ➖ 2

A row can have several letters, e.g. `T S`. "C" is the honest default for anything that is only menu glue.

## File

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| New | ✅ | C | Blank image dialog, uses the background colour |
| Open | ✅ | C | PNG, JPEG, BMP, GIF, TIFF, WebP; reads dpi from PNG/JPEG/BMP |
| Save / Save As | ✅ | T | PNG, JPEG, BMP, GIF; dpi written to PNG/JPEG/BMP (tested); dialogs never clicked |
| Upload to FTP Server | ❌ | – | Greyed out |
| Close | ✅ | C | |
| Acquire / Select Source (scanners, TWAIN) | ❌ | – | Greyed out |
| Print | ✅ | S | GDI printing, verified through "Microsoft Print to PDF" (valid PDF). Never sent to a physical printer; the print dialog was never clicked through |
| Print Preview | ✅ | S | Dialog screenshot checked; page layout logic is unit-tested |
| Page Setup | ✅ | T S | Fit/scale/centre/best-fit/margins/colours/header-footer; own dialog, not the Windows one |
| Send by Email | ❌ | – | Greyed out |
| Set as Wallpaper | ✅ | C | |
| Recent files list | ✅ | C | Count is configurable |
| Exit | ✅ | C | |

## Edit

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Undo / Redo | ✅ | T | Unlimited (the original keeps 10) |
| Cut / Copy / Paste / Clear | ✅ | C | Selection-aware; clipboard path never exercised |
| Paste as New Image | ✅ | C | |
| Copy Format | ❌ | – | Greyed out |

## View

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Full Screen | ✅ | C | |
| Previous / Next File | ✅ | C | |
| Toolbar / Status bar / Drawing tools toggles | ✅ | C | |
| Customize (commands, toolbars, keyboard, menus) | ❌ | – | Greyed out |
| Zoom box (25–400 %, Auto) | ✅ | C | |

## Capture

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Full Screen (monitor under pointer) | ✅ | S | Capture primitive verified headless; the menu/hot-key path was not clicked |
| Virtual Desktop | ✅ | S | Headless capture checked |
| Window or Control | ✅ | T S | Picks the innermost child control under the pointer (buttons, panes), or the whole window from its title bar. Checked with real clicks on Notepad: text area gave the edit control, title bar the window. Controls that are not real child windows (many modern/web apps) fall back to the whole window |
| Entire Page with Scrolling | ✅ | T S | Headless run on a 300-line Notepad document: seamless, ends on the last line. The full GUI path (overlay click → scroll) was not run. Sticky headers/footers and animated areas break stitching (as the manual also warns) |
| Button | ✅ | T S | Two ways: **Button** (pick on a frozen screen) and **Button under Cursor (live)** (captures hover/pressed states). The live path was verified on a real dialog button with margins +2, −1 and 5-left (sizes exact); the frozen-screen picker path was not clicked |
| Active Window | ✅ | S | Headless capture checked |
| Active Window without Frame | ✅ | C | Client area of the foreground window |
| Region (drag or two clicks, magnifier) | ✅ | S | End to end with synthetic mouse input; sizes exact |
| Free Hand | ✅ | T S | End to end; area outside the outline filled with the background colour |
| Pan Last Region | ✅ | S | End to end |
| Multi-Region (preserve/tile, margin) | ✅ | T S | End to end; switching between window/region mode by right-click instead of a menu |
| Mouse Cursor Only | ✅ | S | File produced; content not inspected |
| Repeat Last Capture | ✅ | C | |
| Extended Active Window | ❌ | – | Greyed out |
| Enable Video or Game Capture (DirectX/Glide) | ❌ | – | Greyed out |
| Auto-capture every N seconds (+ Shift+F11 to stop) | ✅ | C | |
| Capture Settings dialog | 🟡 | S | Tabs: Capture, Region, View & Edit, Copy & Print, QuickSave, TextSnap, **Crop & Scale**, **Button**. Crop & Scale verified with a real capture (50x30 crop ×2 = 100x60). Missing: "turn off font smoothing", "clear Vista transparency", paste-to-another-window options |
| Shutter sound | 🟡 | C | Plays the system beep, not a camera sound |
| Hide window before / restore after capture | ✅ | S | Seen working in the TextSnap run |
| QuickSave (auto-increment names, printf names, loop, FTP) | 🟡 | T | Naming logic fully tested; GUI auto-save never run; FTP ❌ |
| Do actions only if the capture changed | ✅ | C | |
| Screen Capture Hot Keys (rebindable) | ✅ | S | Registration and conflict display checked. A real key press triggering a capture was not tested (key injection was blocked); the same code path was driven by a start-up hook |

Default hot keys: manual defaults kept (Ctrl+Shift+R region, W window, M multi-region, S scrolling page, T text, U text under cursor, Shift+F11 stop). The manual gives none for full screen; **Ctrl+Shift+F is my choice**.

## Image

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Crop (to selection) | ✅ | T | |
| Cut out strip (horizontal / vertical) | ✅ | T S | Dialog screenshot checked |
| Change Resolution (dpi) | ✅ | T S | Tag only, as in the manual; affects print size and saved files |
| Scale | ✅ | C | Lanczos resize; dialog never clicked |
| Auto-trim | ✅ | T | |
| Stamps (manager, editor, macros, placement, auto-add) | ✅ | T S | Stamps placed on the picture or at the selection stay objects: click selects, drag moves, corner drag resizes (Shift = keep aspect), double-click edits that instance only, Delete removes it — all checked with a real mouse. Header/footer stamps change the canvas and are applied to the pixels. Manager list/insert/delete via the UI not exercised. Own file format, not `.hs5stamp`; no "Edit Image" button |
| Mirror, Rotate | ✅ | C | |
| Shear | ✅ | T | Dialog never viewed |
| Mosaic | ✅ | T | Dialog never viewed |
| Emboss | ✅ | T | Dialog never viewed |
| Sharpen or Blur | ✅ | T | Dialog never viewed |
| Shadow (manual, auto on captures, on new shapes) | ✅ | T S | |
| Frame (flat/3D, around selection, auto on captures) | ✅ | T S | |

## Color

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Color Resolution (bpp, palettes, dithering) | ✅ | T | Dialog never viewed |
| Black and White, Gray Scale, Invert Colors | ✅ | T | |
| Halftone | ✅ | T | Dialog never viewed |
| Color Correction | ✅ | T | Dialog never viewed; no "apply immediately" switch (preview is always live) |
| Invert Black and White | ✅ | T | |
| Substitute Colors | ✅ | T S | Replace/swap, tolerance, open/save list, Ctrl/Shift multi-select with bulk edit, Apply and Revert (checked with real clicks: Apply → Revert → Apply → Cancel undoes it) |
| Unique Colors | ✅ | T | Counting tested; result dialog never viewed |

## TextSnap

The original reads text through Windows drawing hooks. HSnap uses **UI Automation first, Windows OCR second**, so behaviour differs: it can read text inside bitmaps (the original could not), but cannot read from apps that expose nothing and that OCR misreads.

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Text from a Region | ✅ | S | End to end: overlay → OCR → clipboard → text window → status bar, three lines correct |
| Text from an Object under Cursor | 🟡 | T S | UI Automation extraction verified on a message box and Notepad (via window handle). Picking by pointer position was **not** verified: a screen-wide overlay in this session intercepted hit-testing |
| Text from a window with auto-scroll | 🟡 | S | Scrolling capture and OCR each verified; the combined path was not run |
| Text from a region with auto-scroll | 🟡 | – | Not run. It scrolls the window under the region's centre; the original has you click the scroll-bar arrow |
| Plain / tab-delimited capture | ✅ | T S | Tab mode merges split OCR lines into rows |
| Formatted (RTF) capture | ❌ | – | Neither UIA nor OCR reports fonts |
| Show / append in own window, always to clipboard | ✅ | S | |
| Extra delay for object capture | ✅ | C | |
| Object auto-scroll option | ❌ | – | |
| Quick capture by modifier keys + mouse; right-click line capture | ❌ | – | |
| Hot keys Ctrl+Shift+T / U | ✅ | C | Rebindable |
| Chinese text | 🟡 | – | zh-Hant-TW OCR pack is installed and CJK spacing is fixed in code, but only English was tested |

## Options, Window, Help

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Activate Hot Keys | ✅ | C | |
| Activate Voice Commands / voice command list | ❌ | – | |
| Control Mouse with Keyboard | ❌ | – | |
| Startup and Tray Icon (auto-start, minimised, tray, close-to-tray) | ✅ | T S | Tray icon, hidden start and close-to-tray checked in a real run; registry round trip tested. "For all users" needs admin and was not tried |
| Default Image Resolution | ✅ | S | Inside Change Resolution ("use as default") |
| Image Position and Background | ❌ | – | |
| OfficeXP Style | ❌ | – | |
| Register File Types | ❌ | – | |
| Save / Read Configuration | 🟡 | – | Settings save automatically to `%APPDATA%\hsnap\settings.json`; no export/import commands |
| Window → Close All | ✅ | C | |
| Window → Cascade / Tile / Arrange Icons / Windows… | ➖ | – | Documents are tabs, not MDI windows |
| Help → Contents | 🟡 | – | Opens the installed original `.chm` if present |
| Help → About | ✅ | C | |
| Tutorials, Check for a New Version, Registration and License | ➖ | – | Not applicable to a rewrite |

## Drawing toolbar and common controls

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Select area, Cut, Copy, Paste, Clear, Undo, Redo | ✅ | C | |
| Magnify, View-only (pan) | ✅ | C | |
| Resize button | 🟡 | – | Available as Image → Scale, not on the palette |
| Flood fill | ✅ | T | |
| Add text | 🟡 | T | One font (Segoe UI/Arial), no font dialog, no rich formatting, no "update immediately" option |
| Air brush, Eraser, Freehand, Line | ✅ | C | Never drawn with a real mouse |
| Arrow | ✅ | T S | Hollow, filled and architectural heads, length/width, head at end or start. Filled and architectural drawn with a real mouse; hollow checked by test |
| Rectangle, rounded rect, ellipse (+ filled), Shift = circle | ✅ | T C | Rendering tested; mouse drawing never done |
| Foreground / background colour | 🟡 | C | System colour picker; no 40-colour floating palette, no eyedropper |
| Tool width, eraser width | 🟡 | C | No line-style list or "Smooth lines" switch |
| Toggle Object Shadow | ✅ | T S | Rendering tested; toolbar button visible; not toggled with a mouse |
| Print Direct | ✅ | C | Prints to the default printer — untested on purpose |
| Help ("What is this?") cursor | ❌ | – | |
| Status bar, image view area | ✅ | S | |
| Apply / Revert / Cancel / OK buttons | ✅ | S | All Image/Color filter dialogs have OK, Apply, Revert and Cancel (Cancel also undoes earlier Applies) |

## Other manual topics

| Manual feature | Status | Verified | Notes |
|---|---|---|---|
| Command line usage (`-snap:`, `-save:`, `-QuickSave,…`, `-hidden`, `-iconic`) | ❌ | – | HSnap has its own test switches (`--cli …`, `--pick`) instead; the original syntax is not supported |
| Many file formats (TIFF, PCX, TGA, PSD, EPS, WMF, FAX…) | 🟡 | – | Save: PNG, JPEG, BMP, GIF. Open also reads TIFF/WebP |
| Auto-scroll for the original "Video Player Capture" tutorial | ❌ | – | Needs DirectX/overlay capture |

---

# Never exercised — please test these by hand

Everything marked **C**, plus these gaps in the rows marked T/S:

1. **Drawing with the mouse**: pencil, line, hollow arrow, rectangle, ellipse, text, air brush, eraser, flood fill, selection, Magnify, view-only panning, Shift-for-circle, and the shadow on a freshly drawn shape. (Filled/architectural arrows and stamp objects were done with a real mouse.)
2. **Real hot keys**: pressing Ctrl+Shift+R/W/M/S/T/U/F and Shift+F11. Registration is verified, key presses were not (Ctrl+Shift+S and W were already taken by another program earlier on this machine).
3. **Capture menu / toolbar buttons** for Full Screen, Window, Active, Repeat, Cursor, Auto-capture, and the capture modes' GUI paths (scrolling-page click, active window without frame, the Button frozen-screen picker). Full Screen through a start-up hook, Window or Control through the overlay, and Button live through the command line were checked.
4. **File dialogs**: New, Open, Save (PNG/JPEG/BMP/GIF through the UI), recent files, Set as Wallpaper, Full Screen view, tab switching, zoom box.
5. **Clipboard**: Cut/Copy/Paste/Paste as New Image, auto-copy.
6. **QuickSave** and **auto-print / print dialog** through the GUI; printing on a **physical printer**; Print Direct.
7. **Stamps manager**: list, preview, insert on an open image, delete (Recycle Bin), auto-add on capture. (Moving, resizing and editing an inserted stamp object were checked.)
8. **Filter dialogs never viewed**: Shear, Mosaic, Emboss, Sharpen/Blur, Color Resolution, Halftone, Color Correction, Unique Colors; and clicking OK to apply any filter on a selection (Apply/Revert/Cancel were checked on Substitute Colors only).
9. **Scale, Cut out strip, Crop** applied through the menu.
10. **TextSnap**: pointer-based Text from an Object under Cursor; the two auto-scroll text modes; Chinese text; tab-delimited mode on a real table.
11. **System tray menu** capture items and **auto-start for all users**.
12. **Multi-monitor / mixed DPI**: the selection overlay assumes one DPI for the whole desktop; only the single-DPI case was tried.
