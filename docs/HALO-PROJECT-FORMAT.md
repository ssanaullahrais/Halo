# Halo recording projects — format version 1

Every recording gets its own project folder under `source-files`, with a `.halo`
JSON document and an accompanying `<same name>.assets` directory. Keep the whole
project folder together when copying or moving it. Published MP4s stay in the main
recording folder and are not required to reopen a complete project.

```text
Recordings/
  Recording.mp4
  Recording (original).mp4
  source-files/
    Recording/
      Recording.halo
      Recording.assets/
        segment-0.mp4
        mic-0.wav
        system-0.wav
        cursor-0.jsonl
```

In the recorder, open the source menu (Screen/Window/Area) and choose **Open
project…**. Selecting a project starts a new export through the existing recorder
widget. **Export again** on a saved-recording card uses the same path. Exporting
never changes the user's recording preferences or overwrites a previous export.

## What is retained

- Encoded, unframed video segments, with explicit start times and durations.
- Separate captured microphone and system-audio WAV files, identified by role.
- Cursor position, visibility-in-capture and left/right button samples in JSONL.
- Drawing objects with editable geometry, arrow bends, text, colours, stroke,
  start/end times and their transformation keyframes.
- Frame, background, padding, resolution, encoder preference and quality settings.
- Audio gains and microphone enhancement settings.
- Original-video cache metadata and export state (`recording`, `paused`, `ready`,
  `exporting`, `cancelled`, `failed` or `completed`).

Capture media goes directly into the assets directory. Saving does not copy large
videos from a cache. Generated PNGs, FFmpeg command files and working export
segments remain disposable. Reopening regenerates them from the project data.
Cancelling an export does not cancel or delete the completed source project.

The native OS cursor may already be baked into a video segment; each segment's
`native_cursor_baked` field records this fact. An editor cannot remove those
pixels. Drawings, synthetic cursor effects and window frames remain separate.
Audio that was muted during capture contains silence; a project cannot recover
sound that was not recorded.

## Document contract

The root contains:

| Field | Meaning |
| --- | --- |
| `format` | Always `halo.recording-project` |
| `schema_version` | Integer `1`; unsupported versions are rejected |
| `project_id` | Stable project identifier |
| `revision` | Source-edit revision, initially `1` |
| `created_at`, `updated_at` | UTC RFC 3339 timestamps |
| `app_version` | Halo version that wrote the source |
| `capture` | Source type, physical pixel dimensions and frame rate |
| `render` | Portable render settings; excludes output folder, shortcut and device IDs |
| `timeline` | `time_unit: "seconds"`, total duration and ordered segments |
| `annotations` | Global-timeline annotation objects and keyframes |
| `original` | Optional relative MP4 path and audio-mix cache signature |
| `export` | Latest state, output filename/location and failure summary |

A timeline segment contains `id`, `start`, `duration`, `video`,
`native_cursor_baked`, `audio` and optional `cursor_events`. Audio entries contain
`id`, `role` (`microphone` or `system`) and `file`. Every path is relative to the
`.halo` document's folder, uses forward slashes, and must resolve inside that
folder. Absolute paths, parent traversal and escaping junctions are rejected.

Export metadata records the output's filename and `output_location`:
`recording_folder` for the normal organized layout, or `project_folder` when a
standalone project has been moved elsewhere. The importer does not read source
media through this output metadata. An optional cached original must be inside
the project folder; published originals outside it are rebuilt from the source
segments when needed, without depending on the old recording folder.

Each cursor JSONL line has this shape:

```json
{"time":0.125,"x":320,"y":180,"inside":true,"buttons":1}
```

Cursor times are segment-local seconds. Positions are physical pixels relative
to the capture's top-left corner. `buttons` is a bitmask: `1` left, `2` right,
`3` both. Samples must be chronological. Annotation times and keyframe times are
global timeline seconds. Video frame boundaries are quantized to the chosen fps.

## Future editor integration

Treat segments, audio roles, annotations and cursor samples as source data.
FFmpeg command text is derived output and is deliberately not part of the format.
Increment `revision` when modifying source content. The original's mix signature
includes the revision, segment descriptions, audio gains and enhancement settings;
changing them invalidates that cached mix. A missing or invalidated original is
rebuilt from the retained media before exporting.

Metadata updates use a temporary document and an atomic rename. Export status
updates preserve the editing data. Simultaneous editing and exporting is not
supported in this first recorder integration; the future editor should introduce
revision-specific export snapshots before enabling concurrent edits. Changes to
the document contract require an explicit schema migration; do not reinterpret
an existing schema version silently.

Current validation limits include a 64 MiB JSON document, 100 annotations, 3,600
keyframes per annotation, 10,000 segments and seven days of timeline duration.
Malformed JSON, unsupported versions, missing media, invalid timing and invalid
cursor records produce errors without deleting or rewriting the source media.

Projects are ready to reopen after Finish or a completed pause checkpoint. An
initial `recording` document with no completed segments is a draft, not a complete
crash-recovery recording. This change prepares source data and recorder re-export;
it does not implement the future video editor.
