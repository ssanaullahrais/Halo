# Changelog

## 0.3.5 � 2026-10-31

- Fixed exports getting stuck on Finalizing: a busy Smart Zoom recording could produce a per-chunk camera filter hundreds of KB long, which made ffmpeg's frame-by-frame expression parser stall for minutes.
- Fixed exports repeatedly cycling through every hardware encoder (NVENC, QSV, AMF) before falling back to slow CPU encoding, even on capable GPUs: a transient GPU session-limit error was being misread as a fatal failure instead of a signal to just retry with fewer workers.
- Fixed exports hanging on the final audio mix step on some recordings: switched to Windows' built-in AAC encoder, which is also faster than ffmpeg's own.
- Reworked the Cursor settings tab: fixed missing labels and spacing bugs, split Cursor Motion and Click Effect into their own cards, and moved keystroke options into Behaviour.
- Smart Zoom now sets sensible defaults on enable (1.75x strength, manual control and live preview off) instead of leaving them as they were.
- Click Effect is on by default at Strong intensity.
- Quieted the Smart Zoom interaction sound.

## 0.3.4 � 2026-10-30

- Save the original recording promptly while rendering frames, backgrounds, cursor effects and annotations separately.
- Speed up styled exports with parallel rendering, frame-based cursor updates and shared image preparation, while preserving output quality.
- Keep exports running when the recorder closes, record another take, and manage queued or running jobs through the tray Processes menu with progress and cancellation.
- Show clear Preparing, Exporting and Finalizing stages, detect stalled renderers, and report final completion through the corner alert.
- Save portable .halo source projects and their assets under source-files, with Export again and project reopening for cancelled or completed exports.
- Improve original-video previews, export controls, recording countdowns and live microphone/system-audio meters.
- Automatically install missing supported components at startup, retaining the verified direct FFmpeg download and bounded installer retries.
- Add an automatic-updates switch in About, enabled by default.

## 0.3.3 — 2026-10-29

- **System check installs work reliably.** FFmpeg now downloads directly into Halo's own folder with a verified checksum, with no winget or admin rights needed (winget remains a fallback).
- Install buttons show "Installing…" immediately, explain what is happening, and report success or the exact error. Stalled installs now stop with a clear message instead of waiting forever.

## 0.3.2 — 2026-10-29

- Run multiple file operations at once, with independent progress cards, cancellation, and a live Running submenu in the tray.
- Recording annotations now replay arrow growth, bends, movement, colour, and thickness changes at their recorded times instead of reusing the final shape.
- Cursor mode allows scrolling through empty space; drawing tools block scrolling. Selecting Cursor clears current drawings while preserving what was already recorded.
- Keep the drawing overlay visible when text editing ends or the underlying app gains focus, while retaining access to the recorder controls.
- Drawing starts off for each recording, with a pencil toggle that resets correctly and ignores overlapping clicks.
- Drawing and recorder windows wait for their styles, fonts, and measured layout before appearing, matching the countdown overlay.
- Remove the simulated drawing toolbar from exported videos and its recording setting.
- Check, download, and install updates automatically when Halo is idle, retry failures in the background, and show a single Update & restart action when an update is ready.
- Improve System check feedback with a clear loading state, refresh icon, completion time, and retry feedback.

## 0.3.1 — 2026-10-30

- **Framed recordings** (macOS or Windows window frame): a settings card with picture previews of each frame, a carousel of 22 gradient backgrounds, and a **Mockup size** slider that previews the real margin. The first background is selected for you.
- Framed videos keep your recording's frame rate and start with the screen showing, and are written at much higher quality so text stays sharp.
- Smoother cursor effects: the pointer is sampled far more often, so the highlight, spotlight, trail and click effects follow the mouse without lag. The macOS-style pointer now matches the real cursor size.
- The click effect is off by default. A **Reset to defaults** button in Advanced restores every recorder setting except your shortcut.
- Recorder polish: no flash when the recorder opens, the settings card sits flush under the bar and grows to fit its content, window picker cards show a placeholder while a preview loads, and the "Recording saved" card is tidier.
- Fixed a scrollbar appearing on the countdown card, and a stray close that could hide the next countdown.
- The text tool for on-screen drawing is easier to type with.

## 0.3.0 — 2026-10-29

- Update alert: when a new version has finished downloading while Halo is open, a card appears in the corner with **Restart & update** and **Later**.
- Recorder area selection stays on screen after you draw it; press **Record** in the bar to start, instead of starting automatically.
- Pause and resume from the main recording button, with a **Restart** option while paused.
- Saving shows its progress in the bar, and Finish only appears while a recording is active.
- System audio is off by default. The recorder bar is centered correctly and the drawing tools are cleaner.
- Touchpad holds now open the wheel, options close when the pointer leaves, and the tray wheel stays open until you close it.
- Refreshed README and screenshots.

## 0.2.0 — 2026-10-29

- Redesigned recorder controls with visual pickers for pointer, click effects and highlights, and a live cursor preview.
- Window recording no longer hangs for windows that are not in front; the source list shows window thumbnails.
- An unavailable microphone or system-audio device no longer blocks recording; the toggle switches off automatically.
- Glowing click effect with weak, moderate and strong intensity.
- The recorder window stays centered as its toolbar changes size.
- Branded Windows installer header and sidebar.
- Refreshed README and radial wheel screenshots.

## 0.1.0 — 2026-10-28

First public release.

- Radial wheel and compact menu for quick file actions.
- Conversion, compression, resizing, file tools and optional upload providers.
- Screen, window and region recording with microphone and system audio.
- Drawing tools with animated arrows, boxes, text and blur; selection, moving and resizing.
- Cursor effects and optional macOS or Windows presentation frames, preserving a separate original recording.
- Tray controls, configurable shortcuts and settings.
- Signed automatic update downloads with an explicit Update & restart action.
