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
FFmpeg exports use Above Normal process priority, and NVIDIA encoding remains
available when the GPU probe succeeds. CPU encoding keeps its automatic encoder
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
workers; cancelling stops them all. NVIDIA session-limit initialization failures
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
