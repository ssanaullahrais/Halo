<div align="center">

<img src="app-icon.png" width="80" alt="Halo" />

# Halo

**Quick file actions and screen recording for Windows.**

[Download](https://github.com/ssanaullahrais/Halo/releases/latest) · [Report an issue](https://github.com/ssanaullahrais/Halo/issues) · [Changelog](CHANGELOG.md)

</div>

Hold a file and choose an action from the **radial wheel** or **menu list**. You can also open Halo from the tray, choose an action, then select your file.

## File tools

[![Halo wheel, menus and file actions](docs/images/halo-file-tools-overview.png)](docs/images/halo-file-tools-overview.png)

| Action | Options |
| --- | --- |
| **Compress** | Auto, smallest file, balanced quality, higher quality, custom quality and image target size. Create ZIP archives for files and folders. |
| **Convert** | Images, video, audio, PDFs and supported documents. Available formats are listed below. |
| **Resize** | Custom dimensions, social media, wallpaper, email and percentage presets. |
| **Edit** | Preview, rotate left/right, flip horizontally/vertically, remove metadata, extract audio and set an image as wallpaper. Extract embedded pictures from PDFs. |
| **Files** | Copy, duplicate, copy path, Base64, image-to-text OCR, move, rename, open location, open in a new window and check file type. |
| **More** | Upload and copy a link, create ZIP, extract ZIP, open with another app, create QR code, view properties and move to the Recycle Bin. |

Options adapt to the file you select. Not every action applies to every format.

## Conversion formats

| Type | Open these formats | Convert to |
| --- | --- | --- |
| **Images** | JPG/JPEG/JFIF, PNG, WebP, AVIF, BMP, GIF, TIFF/TIF, ICO | JPG, PNG, WebP, AVIF, BMP, TIFF, GIF, ICO, PDF and an SVG containing the image |
| **Extended images** | PSD, PSB, TGA, DDS, EXR with the optional image engine | Supported image outputs and PDF |
| **Vector images** | SVG | PNG, JPG, WebP, AVIF, BMP, TIFF, GIF, ICO, PDF |
| **Video** | MP4, MOV, MKV, AVI, WebM, M4V, WMV, FLV, 3GP/3GPP, MPEG/MPG, OGV, TS, VOB, RM | MP4, WebM, MKV, MOV, AVI, OGV, GIF, or supported audio formats |
| **Audio** | MP3, WAV, FLAC, OGG/OGA, M4A/M4B, AAC, WMA, Opus, AIFF | MP3, WAV, OGG, FLAC, M4A, AAC, Opus |
| **PDF** | PDF | PNG, JPG, WebP, TIFF, BMP, GIF, AVIF, ICO, TXT, DOCX, HTML, Markdown |
| **Documents** | TXT, Markdown, HTML, DOCX, CSV, LOG, JSON, XML | Available document and image outputs depend on the source and installed engines |

Animated GIFs have separate animation-preserving options. Document conversion depends on the format and installed software. **Settings → System** shows the components available on your PC.

## Screen recording

[![Halo recorder, capture modes, settings and drawing tools](docs/images/halo-recording-overview.png)](docs/images/halo-recording-overview.png)

- Record a **screen, window or selected area** at original resolution, with 30 or 60 fps options.
- Capture **microphone and system audio**, with separate controls.
- **Pause, resume and finish** from the movable recording toolbar.
- Draw **arrows, boxes, text and blur**. Select, move, resize and delete individual drawings.
- Drawing and editing changes appear over time in the saved video.
- Choose the original pointer, highlights, spotlight, fading trail and click effects.
- Add a **macOS or Windows frame** while keeping a separate original video without the frame.

Choose **Area → Record**, drag your region and release to begin. Press **Esc** to cancel selection. Recording quality and smoothness depend on your hardware and graphics driver.

[See every menu and recorder screenshot →](docs/SCREENSHOTS.md)

## Install

Download the **x64 setup EXE** from [Releases](https://github.com/ssanaullahrais/Halo/releases/latest). An MSI is also available.

Requires **64-bit Windows 10/11 and WebView2**. FFmpeg powers video, audio and recording. Optional engines can be checked in **Settings → System**.

Default shortcuts: **Ctrl + Alt + H** for Halo and **Ctrl + Alt + R** for recording. Both are configurable.

## Automatic updates

Halo checks for updates in the background and verifies signed downloads. Open **Settings → About** to check manually or choose **Update & restart**. Installation is blocked while a recording, conversion or upload is running.

Update signatures are separate from Windows publisher certificates. Windows may show an unknown-publisher warning for the installer.

## Privacy

Files and recordings are processed locally. Upload actions send the selected files to your enabled provider. Update checks contact GitHub; optional engine installation downloads the required component.

## Build

```powershell
git clone https://github.com/ssanaullahrais/Halo.git
cd Halo
npm ci
npm run tauri dev
```

Requires Node.js 22, Rust stable and the [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/). See [the release guide](docs/RELEASING.md) for building signed installers.

---

Made by [Sanaullah Rais](https://github.com/ssanaullahrais) · Tauri · Rust · Lucide · Inter
