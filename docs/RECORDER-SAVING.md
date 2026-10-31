# Recorder saving and export

Stopping a recording saves a playable original before rendering optional cursor
effects, annotations, resolution changes, frames and backgrounds. The original
uses the live capture's compatible H.264 stream without rendering its visuals.
Microphone and system audio are mixed into the delivery when present. Paused
segments are trimmed to their recorded durations before joining.

The original is available immediately in the recorder result card, with a
thumbnail, Play original and Open folder. Styled export runs separately. The
final Recording saved state and primary Play video action appear only after the
styled delivery has completed. Cancel export preserves the original and source
project; Export again renders from the saved project.

## Progress

The styled export toolbar shows **Preparing → Exporting → Finalizing**.
An export waiting behind another job shows **Queued** until it actually starts.
`save_stage` reports `preparing`, `encoding` or `finalizing`; `saving` is used for
the original. `save_percent` measures the current stage, not elapsed wall time.
Preparation counts completed graphics assets and graphs and stays at 100% while
the encoder initializes. Exporting begins only when encoded media time advances.
Real progress below one percent displays a numeric fraction such as `0.4%`. Finalizing covers joining audio
and video, closing the MP4 and publishing the destination. No separate Starting
label, small status badge or large exporting card is shown.

Cursor capture retains high frequency semantic events in the source project.
Before export, the renderer compiles the final state visible at each video frame,
groups commands sharing a timestamp and omits unchanged values. This avoids
feeding thousands of redundant intervals to FFmpeg's sendcmd parser while
preserving pointer, click, spotlight and trail appearance at output frame times.

The compositor uses full-colour YUV 4:4:4 internally and converts to the delivery
format only at encoding. Blur computation runs only during its visible interval.
Filter slice parallelism uses up to four logical processors: measured 8- and
20-thread configurations were slower on the reference 38-overlay workload.
FFmpeg exports use normal priority (below normal during capture), and hardware encoding is
available when a real encode probe succeeds. Auto tries NVIDIA NVENC, Intel Quick
Sync and AMD AMF before CPU H.264. Each recording/resume probes the capture size;
styled exports independently probe the final framed/resized dimensions. Probes
are cancelled with their operation and stop after five seconds per GPU. A failed
hardware initialization can try the next encoder; media/filter/delivery errors
and cancellation are reported without an unrelated retry. CPU mode bypasses
hardware probing. Portable projects retain the Auto/CPU preference and choose
from the hardware on the current PC when reopened. CPU encoding keeps its automatic encoder
thread allocation. Export does not change system power plans or recording quality.

Graphics snapshots are prepared concurrently using at most four workers. Equal
SVG snapshots reuse the first rasterized PNG bytes, with independent cache files.
Preparation progress advances only after an image or its copy actually completes.
Its worker limit also accounts for the largest generated image's memory needs,
independently of how many FFmpeg encoder workers the machine can sustain.

Long captures render as frame-aligned sections of at most 15 seconds. Each
section reads the appropriate source range, restores cursor state at its start,
and replays annotation keyframes relative to the same source timeline. Only
annotations overlapping that section enter its filter graph. Sections with the
highest estimated pixel/blur cost start first to reduce the final idle-worker
tail. GPU exports use up to eight workers and CPU exports up to four, limited
further to one worker per four logical processors and by currently available
memory. Filter threads share the available processors across the active workers.
Progress is the sum of rendered media across all sections and never moves
backwards when workers finish out of order. One section failing stops sibling
workers; cancelling stops them all. Hardware session/resource-limit initialization failures
retire an excess worker and requeue its section for the remaining workers. If all
workers hit that limit, a single bounded sequential attempt reports an unavailable
encoder instead of repeatedly starting new workers.
The finished video sections join by packet copy, and original audio is added once
without per-section audio encodes or priming gaps.

## Background Processes

After publishing the original, Halo registers the styled export as an independent
backend job. The tray always contains **Processes (N)**, including an empty state
when nothing is running. Recorder exports share that list with compression,
conversion and resize operations. Clicking a process opens its current progress
card with cancellation; progress cards are not shown automatically for recordings.
Only completed final exports generate the normal bottom-right success alert.

Closing the recorder leaves exports running, and the Record button remains usable
while the previous recording exports. Only one recording export runs at a time;
each can still render its sections in parallel. Additional exports wait in the
queue. Starting a live capture lowers background export CPU priority until the
capture/original-save finishes. Cancelling one queued or running export affects
only that job, never another recording or operation. A failed job is reported and
the queue continues. Originals and source projects remain available for retry.

Each job owns its cancellation, progress and child-process handles. The foreground
recorder follows only its selected job ID, so an older completion cannot overwrite
a new take's state. Pending destinations are reserved without publishing empty
video files. Project metadata updates are serialized and still use atomic writes.
Native tray mutations run on the UI thread with no Rust mutex held across native
menu calls. Percentage changes update existing menu items in place.

`recorder_jobs` returns job snapshots and recent terminal history;
`running_processes` returns active operations; `show_process` opens one process
card; `process_tray_state` reads the installed native menu for diagnostics.
`recorder_cancel_export` cancels the foreground export, while `cancel_action`
accepts any process ID. `recorder_close` hides the widget without cancelling
saved exports or interrupting an original save. Closing an active live take keeps
the existing capture-discard behavior. Quit Halo cancels background exports and
retains the original/project; the in-memory queue does not survive application exit.
Notification preferences are respected, while a manually opened progress card
still receives its terminal state when notifications are disabled.

## Recovery and organization

Recordings are organized as:

```text
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

The `.halo` file contains versioned render settings, annotation keyframes and
relative asset references. The asset directory contains capture sources and
semantic cursor events. Render intermediates stay in the application cache.
Projects can be opened from the recorder source menu and exported after moving
their project folder. See [HALO-PROJECT-FORMAT.md](HALO-PROJECT-FORMAT.md).

## Failure handling

FFmpeg workers are supervised for cancellation, a 30-second first-media startup
deadline, a 120-second media-progress
stall limit, and a total deadline of `max(600 seconds, 20 × media duration + 120
seconds)`. Audio shutdown has a five-second deadline per track. Export failure
retains the original and project for retry.

Deliveries are written to temporary files beside their destinations and renamed
only after successful completion. Transient Windows sharing violations retry
for up to three seconds; permanent failures are reported without replacing a
previous successful delivery. These bounds cover application process failures;
OS and storage failures can still interrupt saving.

## Measured optimization reference

On the reference i9-10850K / RTX 2060 workstation, the same 108.05-second,
1920×1080, 60 FPS project with 38 annotations and 637 keyframes took:

| Pipeline | Total export time |
| --- | ---: |
| Original serial compositor | 218.129 seconds |
| Initial four-worker renderer | 74.520 seconds |
| Parallel image preparation, weighted scheduling and snapshot reuse | 57.722 seconds |
| CPU-only export of the same project | 66.342 seconds |

The optimized GPU output was byte-for-byte identical to the initial four-worker
output: all 6,483 decoded frames, audio and the entire 18,612,408-byte MP4 matched.
Five renderer workers and four preparation workers were fastest in this test;
eight renderer workers or ten preparation workers were slower. Concurrency is
bounded by the CPU/memory rules above, rather than forced to maximum thread counts.
Native cancellation completed in 626 ms during preparation and 881 ms during
encoding, with every worker stopped and the original/project retained. Timings
describe this test machine and project; encoder settings and media content affect
other exports.

## Cinematic cursor and camera

Smart Zoom plans the final export from the completed interaction timeline. A
confirmed click can begin its gentle zoom about 1.36 seconds beforehand and settle 160 ms before the press. A clicked
editable field or dialog uses a modest zoom and stays framed while its focus
remains active, including pauses with no key presses. Enter, Escape, focus changes and
scrolling release that shot; there is no fixed 1.8-second idle hold. Meaningless
background clicks do not create new camera pulses. Nearby related controls
share a shot across short pauses when the pointer stays in the working area. Hover and central cursor jitter never move the camera.

Focus bounds and key activity categories are recorded when Smart Zoom is enabled.
No typed characters, control names, values or URLs are collected for this planner.
Accessibility runs on a separate bounded worker; unsupported controls use modest
pointer-based framing. Providers that report focus after a first key can be
resolved from later events during export. Existing projects without this metadata
retain click-based framing; missing metadata cannot reconstruct past typing.

Smart Zoom always uses cinematic cursor processing. The Off / Smooth Cursor /
Cinematic Cursor selector is only for recordings with Smart Zoom disabled; its
stored choice is preserved while Smart Zoom is on. Smooth Cursor reduces jitter;
Cinematic Cursor also replaces detours with deliberate glides while preserving
interaction points and drag geometry. Both operate at native capture resolution.

The optional bottom-right preview shares the existing capture input and receives
480-pixel frames at 10 FPS. It shows approximately one second behind capture so
it can anticipate recent interactions. It is a framing approximation, not an
exact live export: the final renderer has the entire future event timeline and
motion-blur effects. The preview follows source aspect ratio and selected mockup,
is excluded from screen capture, and can be disabled in settings or hidden with
its close control. Its preference is local and excluded from project render data.

Mac mockups preserve arrow, hand and text cursor changes. Low-level input hooks
have a dedicated message pump, separate from cursor sampling, file writes and
window queries. Capture requests twice the output FPS for sampling headroom.

The private recording runtime provides repeated frames for idle WGC windows.
A 150 ms warmup discards its synthetic startup pictures before the shared
video/input/audio clock begins. Conversion retains its existing runtime.

Camera blur uses seven bilinear shutter samples only while the camera moves;
held shots retain a single sharp cubic transform. The original fifteen cubic
samples took 6.344 s for 20 full-size benchmark frames; the seven-sample
version took 1.687 s on the same workstation. This measures the camera filter,
not total export time; adding cinematic effects still has a rendering cost.

CPU export encoders share the per-worker thread budget instead of each creating
a machine-wide pool. Render workers run at normal priority when idle and below
normal priority during capture, so exports do not outrank desktop interaction.


Any recording can add Interaction sounds (Kenney CC0 click/tick samples and a soft
zoom whoosh) and an independent Light background music option. Both default off
and apply independently of Smart Zoom. Original recordings remain unchanged.
When audio is the only added effect, delivery copies the source video without
starting a visual renderer or video-encoder probe. The final audio pass mixes them with existing microphone/system audio while
packet-copying the rendered video. Sounds do not add work during capture.

Button zoom entry and exit each take 1.2 seconds. Export begins entry before the
confirmed click, settles shortly before it, then pauses briefly at the press before a smooth return. A focused edit returns
after an outside click, submit, focus/navigation change, or scroll. It does not
return merely because typing paused; if focus remains active at the recording
end, the camera keeps its framing through the last frame.
Cinematic cursor glides connect confirmed interaction/hover anchors and discard
unconfirmed detours; intentional drags retain their recorded geometry.

Scrolling stays wide and uses a short, lightweight vertical spatial blur before
cursor, drawing and mockup overlays. It preserves source timestamps and frame
count. It does not use optical flow or synthesize eased intermediate frames;
that prototype was rejected because of export cost. An isolated 1080p/30 FPS
2.6-second filter benchmark added approximately 0.07 seconds on this workstation,
compared with 10.9 seconds for the interpolation prototype at 960px width. These
are filter measurements, not a guarantee of zero overhead or total export speed.


Keystrokes and cursor artwork
----------------------------
Cursor settings include independent Show keystrokes and Kenney cinematic cursor
style switches. Both default off and work without Smart Zoom. Choosing the
Kenney style changes artwork and native cursor replacement, while the existing
motion selector still controls normal-recording movement. Smart Zoom retains
its cinematic motion. Supported Windows cursor states map to arrow, hand, text,
horizontal/vertical/diagonal/all-direction resize, wait, working, unavailable,
crosshair and help artwork. Unknown application-specific cursors fall back to
arrow. The text caret uses Halo's outlined I-beam for readability.

Show keystrokes records opt-in key labels and modifier combinations in a portable
keys-N.jsonl asset, alongside cursor events. Windows layout translation happens
off the low-level hook, including Unicode packet input when supplied by Windows.
The hook only queues bounded events. Events from unrelated foreground windows
are omitted during window recording. UI Automation password detection suppresses
key prompts; unavailable or stale focus checks suppress prompts as well. Custom
password controls must expose their protected status for this check to recognize
them. Turning the option off records no key labels. Original MP4 assets are not
modified, and older projects cannot reconstruct keys that were never captured.

Key appearance offers Dark glass and Light glass. Raised individual caps use
rounded rims, layered highlights, soft depth shadows, and centered white or dark
labels. Shortcut keys have generous spacing around plus symbols inside a translucent
rounded glass wrapper. Recognized shortcuts include a divider and action label.
Labels preserve readable modifier casing and a Windows logo identifies Win.
Mouse buttons include left, right, middle, back and forward, with held modifiers
combined in the same prompt.
The overlay stays in the final bottom-left corner after camera motion and before
recording fades. They do not zoom with the captured screen. Assets are bundled for offline use;
no installation of fonts or downloads is required. The Smart Zoom preview shows
the selected Kenney cursor artwork; key prompts are applied during export.


Keyboard badges use a compact output-relative size. Repeated identical shortcuts
extend the visible hold without flashing. Different shortcuts crossfade over
160 ms with a small vertical motion, using the same global timeline across
worker joins. The action label has open spacing and no vertical separator.


Interaction sound levels are softer (clicks, keys and zoom cues each reduced by
approximately 5-7 dB), with a gentle high-frequency rolloff. The optional added
audio layer ducks under original speech/audio with a short attack and a smooth
320 ms recovery. The original narration branch keeps its gain; only the added
layer is compressed. This audio pass continues to copy the video stream.
