import "@fontsource-variable/inter";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/* ------------------------------------------------------------------ icons (24px, stroked) */
const ICON = {
  record:'<rect x="3" y="4" width="18" height="14" rx="2"/><path d="M8 21h8M12 18v3"/><circle cx="12" cy="11" r="3"/>',
  compress: '<path d="M12 3v11"/><path d="m7 10 5 5 5-5"/><path d="M5 19h14"/>',
  convert: '<path d="M4 8h15"/><path d="m15 4 4 4-4 4"/><path d="M20 16H5"/><path d="m9 12-4 4 4 4"/>',
  resize: '<path d="M6 2v14a2 2 0 0 0 2 2h14"/><path d="M18 22V8a2 2 0 0 0-2-2H2"/>',
  more: '<circle cx="5" cy="12" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="19" cy="12" r="1.6"/>',
  copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  preview: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  open: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  sliders: '<path d="M4 8h9"/><path d="M17 8h3"/><circle cx="15" cy="8" r="2"/><path d="M4 16h3"/><path d="M11 16h9"/><circle cx="9" cy="16" r="2"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="1.6"/><path d="m21 15-4-4L5 21"/>',
  database: '<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M19.1 4.9 17 7M7 17l-2.1 2.1"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  film: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m10 9 5 3-5 3z"/>',
  music: '<path d="M9 18V5l11-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="17" cy="16" r="3"/>',
  rotL: '<path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 4v5h5"/>',
  rotR: '<path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/>',
  move: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M9 13h6"/><path d="m13 10 3 3-3 3"/>',
  rename: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
  trash: '<path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="m19 6-1 14H6L5 6"/>',
  social: '<rect x="3" y="3" width="18" height="18" rx="5"/><circle cx="12" cy="12" r="4"/><circle cx="17.5" cy="6.5" r=".8"/>',
  monitor: '<rect x="2" y="4" width="20" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>',
  mail: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3 7 9 6 9-6"/>',
  percent: '<path d="m19 5-14 14"/><circle cx="7" cy="7" r="2.2"/><circle cx="17" cy="17" r="2.2"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7"/>',
  chev: '<path d="m9 6 6 6-6 6"/>',
  qr: '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><path d="M14 14h3v3h-3zM20 14v.5M14 20h3M20 17v4"/>',
  edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
  repeat: '<path d="M17 2l4 4-4 4"/><path d="M3 11V9a3 3 0 0 1 3-3h15"/><path d="M7 22l-4-4 4-4"/><path d="M21 13v2a3 3 0 0 1-3 3H3"/>',
  extract: '<path d="M12 15V3"/><path d="m7 8 5-5 5 5"/><path d="M5 19h14"/>',
  flipH: '<path d="M12 3v18"/><path d="M8 7 3 12l5 5z"/><path d="m16 7 5 5-5 5z"/>',
  flipV: '<path d="M3 12h18"/><path d="m7 8 5-5 5 5z"/><path d="m7 16 5 5 5-5z"/>',
  shield: '<path d="M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"/><path d="m9 12 2 2 4-4"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><path d="M12 7.5v.5"/>',
  target: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3"/>',
  pathlink: '<path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1"/><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1"/>',
  bolt: '<path d="M13 2 4 14h7l-1 8 9-12h-7z"/>',
  x: '<path d="M6 6l12 12M18 6 6 18"/>',
  link: '<path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1"/><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1"/>',
};
const svg = (name, attrs = "") =>
  `<svg viewBox="0 0 24 24" ${attrs} aria-hidden="true">${ICON[name]}</svg>`;

const KIND_GLYPH = {
  folder:
    '<svg viewBox="0 0 64 64"><path d="M6 16a5 5 0 0 1 5-5h14l6 7h22a5 5 0 0 1 5 5v26a5 5 0 0 1-5 5H11a5 5 0 0 1-5-5z" fill="#ffc93c"/><path d="M6 25h52v25a5 5 0 0 1-5 5H11a5 5 0 0 1-5-5z" fill="#ffdd6e"/></svg>',
  pdf: '<svg viewBox="0 0 64 64"><path d="M14 4h26l12 12v40a4 4 0 0 1-4 4H14a4 4 0 0 1-4-4V8a4 4 0 0 1 4-4z" fill="#e5382f"/><path d="M40 4v12h12z" fill="#ffb3ad"/><text x="32" y="45" font-size="15" font-weight="700" text-anchor="middle" fill="#fff" font-family="Segoe UI,sans-serif">PDF</text></svg>',
  video:
    '<svg viewBox="0 0 64 64"><rect x="6" y="10" width="52" height="44" rx="8" fill="#fff"/><path d="M26 21v22l18-11z" fill="#1d4ed8"/></svg>',
  audio:
    '<svg viewBox="0 0 64 64"><rect x="6" y="6" width="52" height="52" rx="12" fill="#ffd1ec"/><path d="M26 42V20l20-4v22" fill="none" stroke="#c2188a" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/><circle cx="22" cy="42" r="5" fill="#c2188a"/><circle cx="42" cy="38" r="5" fill="#c2188a"/></svg>',
  other:
    '<svg viewBox="0 0 64 64"><path d="M14 4h26l12 12v40a4 4 0 0 1-4 4H14a4 4 0 0 1-4-4V8a4 4 0 0 1 4-4z" fill="#dfe8f7"/><path d="M40 4v12h12z" fill="#9db4da"/><path d="M20 30h24M20 38h24M20 46h16" stroke="#7b93bd" stroke-width="3" stroke-linecap="round"/></svg>',
};

/* ------------------------------------------------------------------ wheel geometry */
const C = 280;
const R_OUT = 262;
const R_IN = 124;
const R_LABEL = 194;

const VERBS = {
  record:{label:"Record",icon:"record"},
  compress: { label: "Compress", icon: "compress" },
  convert: { label: "Convert", icon: "convert" },
  resize: { label: "Resize", icon: "resize" },
  more: { label: "More", icon: "more" },
  copy: { label: "Copy", icon: "copy" },
  preview: { label: "Preview", icon: "preview" },
  edit: { label: "Edit", icon: "edit" },
  files: { label: "Files", icon: "open" },
  extract: { label: "Extract", icon: "extract" },
  open: { label: "Open", icon: "open" },
};
const DIRECT = new Set(["extract","record"]); // clicked once, no list

const polar = (deg, r) => {
  const t = (deg * Math.PI) / 180;
  return [C + r * Math.sin(t), C - r * Math.cos(t)];
};
function wedgePath(center, span) {
  const [x0, y0] = polar(center - span / 2, R_OUT);
  const [x1, y1] = polar(center + span / 2, R_OUT);
  const [x2, y2] = polar(center + span / 2, R_IN);
  const [x3, y3] = polar(center - span / 2, R_IN);
  const big = span > 180 ? 1 : 0;
  return `M${x0} ${y0}A${R_OUT} ${R_OUT} 0 ${big} 1 ${x1} ${y1}L${x2} ${y2}A${R_IN} ${R_IN} 0 ${big} 0 ${x3} ${y3}Z`;
}

/* ------------------------------------------------------------------ state */
const $ = (id) => document.getElementById(id);
const app = $("app");
app.style.setProperty("--user-opacity", `${(Number(localStorage.getItem("halo.opacity")) || 100) / 100}`);
const S = {
  files: [],
  info: null,
  verb: null,
  mode: "wheel",
  progress: 0,
  bytes: 0,
  busyVerb: null,
  expanded: null,
  confirming: null,
  hoverTimer: null,
  closeTimer: null,
  runId: 0,
  style: "wheel",
};

const kind = () => (S.files.length && S.files.every((f) => f.kind === S.files[0].kind) ? S.files[0].kind : "other");
const totalSize = () => S.files.reduce((n, f) => n + f.size, 0);
const rawVerbList = () => {
  if (!S.files.length) return ["compress", "convert", "resize", "edit", "files", "more"]; // empty drop zone: a preview of the wheel
  const k = kind();
  const every = (...kinds) => S.files.every((f) => kinds.includes(f.kind));
  if (k === "folder") {
    const inside = S.scan && S.files.length === 1 ? S.scan.images + S.scan.videos + S.scan.audio + S.scan.docs : 0;
    return inside > 0 ? ["compress", "convert", "resize", "files", "more"] : ["compress", "files", "more"];
  }
  if (every("image", "video")) return ["compress", "convert", "resize", "edit", "files", "more"];
  if (every("audio")) return ["convert", "compress", "edit", "files", "more"];
  if (every("pdf", "doc", "svg")) return ["convert", "edit", "files", "more"];
  if (k === "archive") return ["extract", "edit", "files", "more"];
  return ["edit", "files", "more"]; // documents and everything else: no pretend conversions
};
const verbList = () => [...rawVerbList().filter((verb) => !S.files.length || DIRECT.has(verb) || options(verb).some((o) => !o.disabled && !o.current)),"record"];

function fmtCount(n) {
  return n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}K` : String(n);
}
function fmtBytes(n) {
  if (!n) return "0 KB";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 100 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

function setMode(mode) {
  S.mode = mode;
  app.dataset.mode = mode;
}

/* ------------------------------------------------------------------ rendering: wheel + hub */
function renderWheel() {
  const verbs = verbList();
  const step = 360 / verbs.length;
  const wedges = verbs
    .map((v, i) => {
      const angle = i * step;
      const [lx, ly] = polar(angle, R_LABEL);
      return `<g class="wedge" data-verb="${v}">
        <path d="${wedgePath(angle, step - 4)}"/>
        <svg class="glyph" x="${lx - 21}" y="${ly - 40}" width="42" height="42" viewBox="0 0 24 24">${ICON[VERBS[v].icon]}</svg>
        <text x="${lx}" y="${ly + 28}">${VERBS[v].label}</text>
      </g>`;
    })
    .join("");

  $("wheel").innerHTML = `
    <div class="shadow"></div>
    <svg viewBox="0 0 560 560">
      <defs>
        <linearGradient id="gradDisc" x1="0" y1="0" x2="0" y2="1"><stop class="stop-a" offset="0"/><stop class="stop-b" offset="1"/></linearGradient>
        <radialGradient id="gradSheen" cx="0.35" cy="0.18" r="0.75"><stop offset="0" stop-color="#fff" stop-opacity="0.34"/><stop offset="0.55" stop-color="#fff" stop-opacity="0.04"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient>
        <linearGradient id="gradActive" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="#8167ff"/><stop offset="1" stop-color="#3f2bd9"/>
        </linearGradient>
      </defs>
      <circle class="disc" cx="${C}" cy="${C}" r="${R_OUT + 10}"/>
      <circle class="sheen" cx="${C}" cy="${C}" r="${R_OUT + 10}"/>
      ${wedges}
    </svg>
    <div class="hub" id="hub"></div>`;
  renderHub();

  $("wheel").querySelectorAll(".wedge").forEach((el) => {
    const verb = el.dataset.verb;
    if (hoverOpensOptions()) {
      el.addEventListener("pointerenter", () => hoverVerb(verb));
      el.addEventListener("pointerleave", () => clearHoverTarget(verb));
    }
    el.addEventListener("click", () => clickVerb(verb));
  });
}

function glyphFor() {
  const f = S.files[0];
  if (kind() === "folder") return KIND_GLYPH.folder;
  if (kind() === "video") return KIND_GLYPH.video;
  if (kind() === "audio") return KIND_GLYPH.audio;
  if (f && f.ext === "PDF") return KIND_GLYPH.pdf;
  return KIND_GLYPH.other;
}

function thumbHtml(extraClass = "") {
  if (S.info && S.info.data) {
    return `<div class="thumb ${extraClass}" style="background-image:url('${S.info.data}')"></div>`;
  }
  return `<div class="thumb glyph-only ${extraClass}">${glyphFor()}</div>`;
}

function closeHintSeen() {
  try { return localStorage.getItem("halo.closeTipDone") === "1"; } catch { return false; }
}

function renderHub() {
  const hub = $("hub");
  if (!hub) return;
  if (!S.files.length) {
    $("wheel").querySelector(":scope > .drop-close")?.remove();
    hub.innerHTML = `<button type="button" class="btn drop-close" aria-label="Close Halo">${svg("x")}</button><div class="drop-hint"><svg viewBox="0 0 24 24"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z"/></svg><div class="hub-name">Select a file</div><div class="hub-meta">Hold a file, or Ctrl + click it</div><div class="hub-meta">Right-click to close</div></div>`;
    const close = hub.querySelector(".drop-close");
    close.onclick = (e) => {
      e.stopPropagation();
      closeWidget();
    };
    if (S.style !== "menu") $("wheel").appendChild(close);
    hub.onclick = () => invoke("browse_files");
    return;
  }
  hub.onclick = null;
  const f = S.files[0];
  const many = S.files.length > 1;
  const name = many ? `${S.files.length} files` : f.name;
  const folderLine = S.scan
    ? [S.scan.images && `${fmtCount(S.scan.images)} photos`, S.scan.videos && `${fmtCount(S.scan.videos)} videos`, S.scan.audio && `${fmtCount(S.scan.audio)} audio`].filter(Boolean).join(" · ")
    : "";
  const meta = many ? fmtBytes(totalSize()) : kind() === "folder" ? folderLine || "Folder" : `${f.size ? fmtBytes(f.size) : ""} • ${f.ext || "FILE"}`;
  hub.innerHTML = `${thumbHtml(many ? "stack" : "")}<div class="hub-name" title="${escapeHtml(name)}">${escapeHtml(name)}</div><div class="hub-meta">${escapeHtml(meta)}</div>`;
}

const escapeHtml = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

function highlight(verb) {
  S.verb = verb;
  document.querySelectorAll(".wedge, .mi").forEach((el) => el.classList.toggle("active", el.dataset.verb === verb));
}

/* ------------------------------------------------------------------ option lists */
// "Repeat last" remembers the settings used the previous time for compress, convert and resize.
const lastKey = (verb) => `halo.last.${verb}`;
function readLast(verb) {
  try {
    return JSON.parse(localStorage.getItem(lastKey(verb)) || "null");
  } catch {
    return null;
  }
}
function saveLast(p) {
  try {
    localStorage.setItem(lastKey(p.verb), JSON.stringify({ option: p.option, value: p.value ?? null, value2: p.value2 ?? null, label: p.label || p.option }));
  } catch {
    /* storage unavailable: skip */
  }
}

function options(verb) {
  const list = rawOptions(verb);
  if (verb === "convert" && S.files.length && S.files.every(f => ["image","video","audio"].includes(f.kind))) {
    const current = f => ({jpeg:"jpg",jfif:"jpg",tif:"tiff"})[f.ext.toLowerCase()] || f.ext.toLowerCase();
    for (const p of conversionPresets) {
      if (S.files.every(f => f.capabilities?.convert?.includes(p.target) || (current(f) === p.target && current(f)!=="gif"))) {
        list.unshift({ id: `preset_${p.id}`, title:p.name, sub:`${p.target.toUpperCase()} · saved preset`, icon:"sliders", verb:"preset", option:p.id, presetTarget:p.target });
      }
    }
  }
  if (!S.files.length || S.files.some((f) => !f.capabilities)) return list;
  const allowed = (option) => S.files.every((f) => f.capabilities[verb]?.includes(option)) &&
    !(S.files.length === 1 && S.scan?.capabilities?.[verb] && !S.scan.capabilities[verb].includes(option));
  const filtered = list.filter((o) => o.disabled || o.current || o.presetTarget || allowed(o.last?.option ?? o.id));
  if (verb === "compress" && !filtered.length && allowed("zip")) {
    return [{ id: "zip", title: S.files.length > 1 ? "One zip each" : "ZIP", sub: "Pack into a ZIP file", icon: "compress" }];
  }
  return filtered;
}

function rawOptions(verb, fs = S.files) {
  if (!fs.length) {
    const seen = new Set();
    return ["image", "video", "audio", "pdf", "doc", "svg", "archive", "other"].flatMap(kind => rawOptions(verb, [{kind, ext: "__preview", name: "", size: 0}]))
      .filter(o => !o.disabled && !o.current && o.id !== "__last" && !seen.has(o.id) && seen.add(o.id));
  }
  const many = fs.length > 1;
  const k = fs.every(f => f.kind === fs[0].kind) ? fs[0].kind : "other"; // one kind when every file is the same type, otherwise "other"
  const every = (...kinds) => fs.every((f) => kinds.includes(f.kind));
  const allMedia = every("image", "video");
  const norm = (e) => ({ jpeg: "jpg", jfif:"jpg", htm: "html", markdown: "md", tif: "tiff" })[e.toLowerCase()] || e.toLowerCase();
  const isCurrent = (id) => fs.every((f) => norm(f.ext) === id); // ticked only when every file already is that format
  const withLast = (list) => {
    const last = readLast(verb);
    const hit = last && list.find((o) => (o.option ?? o.id) === last.option && !o.current && !o.disabled);
    return hit ? [{ id: "__last", title: "Repeat last", sub: last.label, icon: "repeat", last }, ...list] : list;
  };

  const sc = k === "folder" && !many ? S.scan : null; // what is inside the folder
  const folderMedia = sc && sc.images + sc.videos + sc.audio + sc.docs > 0;

  if (verb === "compress") {
    if (folderMedia)
      return [
        { id: "auto", title: "Auto (Recommended)", sub: "Compress the media inside", icon: "bolt" },
        { id: "small", title: "Smallest files", sub: "Best for sharing", icon: "sliders" },
        { id: "medium", title: "Balanced quality", sub: "Good for most uses", icon: "image" },
        { id: "large", title: "Higher quality", sub: "Lower compression", icon: "database" },
        { id: "custom", title: "Custom quality", sub: "Set your own quality", icon: "gear", editor: "quality" },
        { id: "zip", title: "Zip the folder", sub: "Pack everything into a .zip", icon: "compress" },
      ];
    if (allMedia) {
      const list = [
        { id: "auto", title: "Auto (Recommended)", sub: "Best balance of size & quality", icon: "bolt" },
        { id: "small", title: "Smallest file", sub: "Best for sharing", icon: "sliders" },
        { id: "medium", title: "Balanced quality", sub: "Good for most uses", icon: "image" },
        { id: "large", title: "Higher quality", sub: "Lower compression", icon: "database" },
        { id: "custom", title: "Custom quality", sub: "Set your own quality", icon: "gear", editor: "quality" },
      ];
      if (every("image")) list.push({ id: "target", title: "Target size", sub: "Aim for a size you choose", icon: "target", editor: "target" });
      return withLast(list);
    }
    if (every("audio"))
      return withLast([
        { id: "small", title: "Smallest file", sub: "64 kbps", icon: "sliders" },
        { id: "medium", title: "Balanced quality", sub: "128 kbps", icon: "image" },
        { id: "large", title: "Higher quality", sub: "192 kbps", icon: "database" },
        { id: "custom", title: "Custom quality", sub: "Set your own quality", icon: "gear", editor: "quality" },
      ]);
    if (k === "archive") return [{ id: "none", title: "Already compressed", sub: "A zip file can't be made smaller", icon: "compress", disabled: true }];
    const zips = [{ id: "zip", title: many ? "One zip each" : "Zip", sub: many ? "A separate .zip for every file" : "Pack into a .zip file", icon: "compress" }];
    if (many) zips.unshift({ id: "zip_all", title: "One zip for all", sub: `Pack all ${fs.length} files together`, icon: "database" });
    return zips;
  }

  if (verb === "convert") {
    if (folderMedia) {
      if (sc.capabilities) return sc.capabilities.convert.map((id) => ({ id, title: id.toUpperCase(), sub: "Matching files inside", icon: ["mp3","wav","ogg","flac","m4a","aac","opus"].includes(id) ? "music" : ["mp4","webm","mkv","mov","avi"].includes(id) ? "film" : "file" }));
      const rows = [];
      if (sc.images) rows.push(["jpg", "JPG", "Pictures inside", "image"], ["png", "PNG", "Pictures inside", "image"], ["webp", "WEBP", "Pictures inside", "image"], ["avif", "AVIF", "Pictures inside", "image"], ["pdf", "PDF", "Pictures inside", "file"]);
      if (sc.videos) rows.push(["mp4", "MP4", "Videos inside", "film"], ["webm", "WEBM", "Videos inside", "film"], ["mp3", "MP3", "Audio of videos", "music"]);
      if (sc.audio) rows.push(["mp3", "MP3", "Audio inside", "music"], ["wav", "WAV", "Audio inside", "music"], ["ogg", "OGG", "Audio inside", "music"]);
      const seen = new Set();
      return rows.filter(([id]) => !seen.has(id) && seen.add(id)).map(([id, title, sub, icon]) => ({ id, title, sub, icon }));
    }
    if (new Set(fs.map((f) => f.kind)).size > 1)
      return [{ id: "none", title: "Pick one type", sub: "Select only images, videos or audio", icon: "convert", disabled: true }];
    const make = (rows) => rows.map(([id, title, sub, icon]) => ({ id, title, sub, icon, current: isCurrent(id) }));
    if (k === "image")
      return withLast(
        make([
          ["jpg", "JPG", "Small and universal", "image"],
          ["png", "PNG", "Sharp, keeps transparency", "image"],
          ["webp", "WEBP", "Modern and compact", "image"],
          ["avif", "AVIF", "Very small, high quality", "image"],
          ["bmp", "BMP", "Uncompressed", "image"],
          ["tiff", "TIFF", "For print and scans", "image"],
          ["gif", "GIF", "Simple pictures", "image"],
          ["ico", "ICO", "Icon, up to 256 px", "image"],
          ["pdf", "PDF", "One page document", "file"],
          ["svg", "SVG", "Embedded in a vector file", "file"],
          ...(fs.every(f=>f.ext.toLowerCase()==="gif") ? [
            ["mp4","MP4","Keep the animation as video","film"], ["webm","WEBM","Animated video for the web","film"],
            ["mkv","MKV","Keep the animation as video","film"], ["mov","MOV","Keep the animation as video","film"],
            ["avi","AVI","Keep the animation as video","film"], ["ogv","OGV","Open Theora animation","film"],
          ] : []),
        ]),
      );
    if (k === "video")
      return withLast(
        make([
          ["mp4", "MP4", "Plays everywhere", "film"],
          ["webm", "WEBM", "Great for the web", "film"],
          ["mkv", "MKV", "Flexible container", "film"],
          ["mov", "MOV", "Apple QuickTime", "film"],
          ["avi", "AVI", "Older Windows video", "film"],
          ["ogv", "OGV", "Open Theora video", "film"],
          ["gif", "GIF", "Short looping clip", "image"],
          ["mp3", "MP3", "Audio only", "music"],
          ["wav", "WAV", "Uncompressed audio", "music"],
          ["m4a", "M4A", "Audio only, AAC", "music"],
          ["aac", "AAC", "Audio only", "music"],
          ["ogg", "OGG", "Audio only, Vorbis", "music"],
          ["flac", "FLAC", "Lossless audio extraction", "music"],
          ["opus", "OPUS", "Efficient audio", "music"],
        ]),
      );
    if (k === "audio")
      return withLast(
        make([
          ["mp3", "MP3", "Plays everywhere", "music"],
          ["wav", "WAV", "Uncompressed", "music"],
          ["ogg", "OGG", "Open and compact", "music"],
          ["flac", "FLAC", "Lossless", "music"],
          ["m4a", "M4A", "AAC audio", "music"],
          ["aac", "AAC", "Compact audio", "music"],
          ["opus", "OPUS", "Efficient for voice", "music"],
        ]),
      );
    if (k === "pdf")
      return withLast(
        make([
          ["png", "PNG", "Each page as a picture", "image"],
          ["jpg", "JPG", "Each page, small files", "image"],
          ["webp", "WEBP", "Each page, compact", "image"],
          ["tiff", "TIFF", "Each page, for print", "image"],
          ["bmp", "BMP", "Each page, uncompressed", "image"],
          ["gif", "GIF", "Each page, simple", "image"],
          ["avif", "AVIF", "Each page, very small", "image"],
          ["ico", "ICO", "Each page as an icon", "image"],
          ["txt", "TXT", "The text only", "file"],
          ["docx", "DOCX", "Text you can edit in Word", "file"],
          ["html", "HTML", "The text as a web page", "file"],
          ["md", "MD", "The text as Markdown", "file"],
        ]),
      );
    if (k === "svg")
      return withLast(
        make([
          ["png", "PNG", "Sharp, keeps transparency", "image"],
          ["jpg", "JPG", "Small and universal", "image"],
          ["webp", "WEBP", "Modern and compact", "image"],
          ["avif", "AVIF", "Very small", "image"],
          ["bmp", "BMP", "Uncompressed", "image"],
          ["tiff", "TIFF", "For print", "image"],
          ["gif", "GIF", "Simple pictures", "image"],
          ["ico", "ICO", "Icon, up to 256 px", "image"],
          ["pdf", "PDF", "A one page document", "file"],
        ]),
      );
    if (k === "doc") {
      const src = norm(fs[0].ext);
      const rows = [
        ["pdf", "PDF", "Ready to share or print", "file"],
        ["docx", "DOCX", "Opens in Word", "file"],
        ["html", "HTML", "A web page", "file"],
        ["txt", "TXT", "Plain text", "file"],
        ["md", "MD", "Markdown", "file"],
        ["png", "PNG", "Each page as a picture", "image"],
        ["jpg", "JPG", "Each page as a picture", "image"],
        ["webp", "WEBP", "Each page, compact", "image"],
        ["avif", "AVIF", "Each page, very small", "image"],
        ["bmp", "BMP", "Each page, uncompressed", "image"],
        ["tiff", "TIFF", "Each page, for print", "image"],
        ["gif", "GIF", "Each page, simple", "image"],
        ["ico", "ICO", "Each page as an icon", "image"],
      ].filter(([id]) => id !== src);
      return withLast(make(rows));
    }
    return [{ id: "none", title: "Not available", sub: "Convert works on pictures, videos, audio, PDFs and documents", icon: "convert", disabled: true }];
  }

  if (verb === "resize") {
    if (folderMedia && sc.images + sc.videos > 0)
      return [
        { id: "social", title: "Social Media", sub: "1080 × 1080", icon: "social" },
        { id: "wallpaper", title: "Wallpaper", sub: "1920 × 1080", icon: "monitor" },
        { id: "email", title: "Email", sub: "800 × 600", icon: "mail" },
        { id: "percent", title: "Percentage", sub: "50%", icon: "percent", editor: "percent" },
      ];
    if (allMedia)
      return withLast([
        // An exact width and height only makes sense for one picture, so it is left out for a selection.
        ...(many ? [] : [{ id: "custom", title: "Custom Size", sub: "Set width & height", icon: "resize", editor: "size" }]),
        { id: "social", title: "Social Media", sub: "1080 × 1080", icon: "social" },
        { id: "wallpaper", title: "Wallpaper", sub: "1920 × 1080", icon: "monitor" },
        { id: "email", title: "Email", sub: "800 × 600", icon: "mail" },
        { id: "percent", title: "Percentage", sub: "50%", icon: "percent", editor: "percent" },
      ]);
    return [{ id: "none", title: "Not available", sub: "Resize works on images and videos", icon: "resize", disabled: true }];
  }

  if (verb === "edit") {
    const list = [{ id: "preview", title: "Preview file", sub: many ? "Opens the first file" : "Open it in its usual app", icon: "preview" }];
    if (every("pdf")) list.push({ id: "extract_images", title: "Extract images", sub: "Save embedded pictures at native size", icon: "image" });
    if (every("image")) {
      list.push({ id: "rotate_left", title: "Rotate Left", sub: "Turn it 90° counter-clockwise", icon: "rotL" });
      list.push({ id: "rotate_right", title: "Rotate Right", sub: "Turn it 90° clockwise", icon: "rotR" });
      list.push({ id: "flip_h", title: "Flip Horizontal", sub: "Mirror left to right", icon: "flipH" });
      list.push({ id: "flip_v", title: "Flip Vertical", sub: "Mirror top to bottom", icon: "flipV" });
    }
    if (!many && every("image")) list.push({ id: "set_wallpaper", title: "Set as wallpaper", sub: "Use it as your desktop background", icon: "monitor" });
    if (every("video")) list.push({ id: "extract_audio", verb: "convert", option: "mp3", title: "Extract audio", sub: "Save the sound as an MP3", icon: "music" });
    if (every("image", "video", "audio")) list.push({ id: "strip_meta", title: "Remove metadata", sub: "Saves a copy without hidden details", icon: "shield" });
    if (!many && !every("image", "video", "audio") && k !== "folder") list.push({ id: "open_with", title: "Open with…", sub: "Choose another app", icon: "open" });
    return list;
  }

  if (verb === "files") {
    const list = [];
    if (k === "folder" && !many) list.push({ id: "open", title: "Open", sub: "Show what's inside", icon: "open" });
    list.push({ id: "copy", title: "Copy file", sub: "Paste it anywhere", icon: "copy" });
    list.push({ id: "duplicate", title: "Duplicate", sub: "Make a copy beside it", icon: "copy" });
    list.push({ id: "copy_path", title: "Copy file path", sub: "Copy where it is stored", icon: "pathlink" });
    if (!many && k !== "folder") list.push({ id: "copy_base64", title: "Copy as Base64", sub: "Copy the file's data as text", icon: "file" });
    if (!many && every("image")) list.push({ id: "copy_ocr", title: "Copy as text (OCR)", sub: "Read the words in the picture", icon: "preview" });
    list.push({ id: "move_to", title: "Move to…", sub: "Choose a new folder", icon: "move" });
    if (!many) list.push({ id: "rename", title: "Rename", sub: "Give it a new name", icon: "rename", editor: "rename" });
    list.push({ id: "open_location", title: "Open location", sub: "Show it in its folder", icon: "open" });
    if (!many) list.push({ id: "show_new_window", title: "Show in new window", sub: "Open its folder in a new window", icon: "open" });
    if (!many) list.push({ id: "check_type", title: "Check file type", sub: "See what the file really is", icon: "info" });
    return list;
  }

  if (verb === "more") {
    const list = [];
    if (k !== "folder") list.push({ id: "share_link", verb: "share", option: "link", title: "Upload & get link", sub: many ? "Uploads copies, then copies the links" : "Uploads a copy, then copies the link", icon: "pathlink" });
    if (k !== "folder" && k !== "archive") list.push({ id: "zip_it", verb: "compress", option: many ? "zip_all" : "zip", title: "Create ZIP", sub: many ? "Pack them into one zip" : "Pack it into a zip", icon: "compress" });
    if (k === "archive" && !many) list.push({ id: "extract", title: "Extract", sub: "Unpack into a new folder", icon: "extract" });
    if (!many && k !== "folder") list.push({ id: "open_with", title: "Open with…", sub: "Choose another app", icon: "open" });
    if (!many && k !== "folder") list.push({ id: "qr", title: "Create QR code", sub: "A QR picture that links to the file", icon: "qr" });
    if (!many) list.push({ id: "properties", title: "Properties", sub: "Size, dates and details", icon: "info" });
    list.push({ id: "delete", title: "Delete", sub: "Move to the Recycle Bin", icon: "trash", danger: true, confirm: true });
    return list;
  }
  return [];
}
/* ------------------------------------------------------------------ cards */
const FLYOUT_FROM = 7; // a list this long or longer becomes one compact scrolling panel

function renderCards(verb) {
  const holder = $("cards");
  S.expanded = null;
  S.confirming = null;
  const list = verb && !DIRECT.has(verb) ? options(verb) : [];
  const n = list.length;
  const asFlyout = n >= FLYOUT_FROM && S.style !== "menu"; // the menu theme already has its own flyout treatment
  holder.classList.toggle("flyout", asFlyout);
  holder.classList.toggle("dense", n > 7 && !asFlyout);

  holder.innerHTML = "";
  const target = asFlyout ? holder.appendChild(Object.assign(document.createElement("div"), { className: "flyout-box" })) : holder;

  // Every card keeps the same gap from the wheel, regardless of its position in the list.
  list.forEach((opt, i) => {
    const dx = 0;
    const el = document.createElement("div");
    el.className = "card" + (opt.disabled ? " disabled" : "") + (opt.current ? " current" : "") + (opt.danger ? " danger" : "");
    el.dataset.option = opt.id;
    el.style.setProperty("--i", i);
    el.style.setProperty("--dx", `${dx}px`);
    el.innerHTML = cardInner(opt);
    el.addEventListener("click", () => onCard(verb, opt, el));
    target.appendChild(el);
  });

  placeFlyout(verb);
  fitWidget();
}

// Colourful icon tiles: each option gets a colour from its meaning.
const PALETTE = {
  blue: ["#5b9bff", "#2b6ae0"], purple: ["#b08cf8", "#7b4fe0"], green: ["#4fd183", "#22a65a"], orange: ["#ffae52", "#f07a1a"],
  red: ["#ff7676", "#e5384c"], pink: ["#ff82bf", "#e5459a"], teal: ["#45d2e0", "#1b9fb8"], indigo: ["#7d89f9", "#4452d9"],
  amber: ["#ffd04f", "#f0a21a"], gray: ["#a0aec0", "#6b7a90"],
};
const TONE_BY_ID = {
  auto: "amber", small: "green", medium: "blue", large: "purple", custom: "gray", target: "red", zip: "orange", zip_all: "orange", zip_it: "orange",
  jpg: "blue", png: "purple", webp: "teal", avif: "green", bmp: "orange", tiff: "red", gif: "pink", ico: "indigo", pdf: "red", svg: "amber",
  mp4: "blue", webm: "teal", mkv: "purple", mov: "orange", avi: "red", mp3: "pink", wav: "indigo", m4a: "purple", aac: "teal", opus: "green", ogg: "green", flac: "amber",
  docx: "blue", html: "orange", txt: "gray", md: "purple",
  social: "pink", wallpaper: "blue", email: "amber", percent: "green",
  preview: "teal", rotate_left: "blue", rotate_right: "indigo", flip_h: "purple", flip_v: "pink", extract_audio: "pink", strip_meta: "green", open_with: "orange",
  copy: "blue", duplicate: "indigo", copy_path: "teal", copy_base64: "purple", copy_ocr: "orange", move_to: "amber", rename: "green", open_location: "blue", open: "amber",
  share_link: "green", qr: "purple", set_wallpaper: "blue", check_type: "teal", show_new_window: "indigo", extract: "amber", properties: "gray", delete: "red", __last: "indigo",
};
const TONE_BY_VERB = { compress: "green", convert: "blue", resize: "purple", edit: "teal", files: "amber", more: "gray", extract: "orange", preview: "teal", open: "amber", copy: "blue" };
const toneStyle = (name) => {
  const [a, b] = PALETTE[name] || PALETTE.blue;
  return `--c1:${a};--c2:${b}`;
};

function cardInner(opt) {
  return `<span class="ico" style="${toneStyle(TONE_BY_ID[opt.id])}">${svg(opt.icon)}</span>
    <span class="txt"><div class="title">${escapeHtml(opt.title)}</div><div class="sub">${escapeHtml(opt.sub)}</div></span>
    ${opt.current ? `<span class="tick">${svg("check")}</span>` : ""}`;
}

function onCard(verb, opt, el) {
  if (opt.disabled || opt.current) return;
  if (opt.editor === "rename" && !S.files.length) return invoke("browse_files", {verb, action:{verb, option:opt.id, value:null, extra:{editor:opt.editor}}});
  if (opt.editor) return openEditor(verb, opt, el);
  if (opt.confirm) {
    if (S.confirming !== opt.id) {
      S.confirming = opt.id;
      el.classList.add("confirming");
      el.querySelector(".title").textContent = "Click again to delete";
      el.querySelector(".sub").textContent = "It goes to the Recycle Bin";
      return;
    }
  }
  if (opt.last) return perform(verb, opt.last.option, { value: opt.last.value, value2: opt.last.value2, label: opt.last.label });
  perform(opt.verb ?? verb, opt.option ?? opt.id, { label: opt.title });
}

function openEditor(verb, opt, el) {
  document.querySelectorAll(".card.editor").forEach((c) => c.classList.remove("editor"));
  S.expanded = opt.id;
  el.classList.add("editor");
  const head = `<div class="editor-head"><span class="ico">${svg(opt.icon)}</span><div class="title">${escapeHtml(opt.title)}</div><button class="apply">${opt.editor === "rename" ? "Rename" : "Apply"}</button></div>`;
  const stop = (e) => e.stopPropagation();
  const paintAll = () =>
    el.querySelectorAll('input[type="range"]').forEach((r) => r.style.setProperty("--p", `${((r.value - r.min) / (r.max - r.min)) * 100}%`));

  if (opt.editor === "quality" || opt.editor === "percent") {
    const isQ = opt.editor === "quality";
    const [min, max, def] = isQ ? [10, 95, 70] : [10, 100, 50];
    el.innerHTML = `${head}
      <div class="row"><label>${isQ ? "Quality" : "Size"}</label><input type="range" min="${min}" max="${max}" value="${def}"/><output>${def}%</output></div>`;
    const range = el.querySelector("input");
    const out = el.querySelector("output");
    range.addEventListener("input", () => {
      out.textContent = `${range.value}%`;
      paintAll();
    });
    el.querySelector(".apply").addEventListener("click", (e) => {
      stop(e);
      perform(verb, opt.id, { value: Number(range.value), label: `${opt.title} · ${range.value}%` });
    });
  } else if (opt.editor === "size") {
    const w0 = S.info?.w || S.files[0]?.media?.w || 1920;
    const h0 = S.info?.h || S.files[0]?.media?.h || Math.round((w0 * 9) / 16);
    const isIcon = S.files.length > 0 && S.files.every((f) => f.ext.toLowerCase() === "ico");
    const maxW = isIcon ? Math.min(256, w0 * 2) : w0 * 2;
    const maxH = isIcon ? Math.min(256, h0 * 2) : h0 * 2;
    const ratio = h0 / w0;
    let lock = true;
    el.innerHTML = `${head}
      <div class="row"><label>Width</label><input id="rw" type="range" min="16" max="${maxW}" value="${w0}"/><output id="ow" class="px">${w0}</output></div>
      <div class="row"><label>Height</label><input id="rh" type="range" min="16" max="${maxH}" value="${h0}"/><output id="oh" class="px">${h0}</output></div>
      <div class="actions"><button class="chip on" id="lock">Keep proportions</button></div>`;
    const rw = el.querySelector("#rw"), rh = el.querySelector("#rh"), ow = el.querySelector("#ow"), oh = el.querySelector("#oh");
    rw.addEventListener("input", () => {
      if (lock) rh.value = Math.max(16, Math.round(rw.value * ratio));
      ow.textContent = rw.value;
      oh.textContent = rh.value;
      paintAll();
    });
    rh.addEventListener("input", () => {
      if (lock) rw.value = Math.max(16, Math.round(rh.value / ratio));
      ow.textContent = rw.value;
      oh.textContent = rh.value;
      paintAll();
    });
    el.querySelector("#lock").addEventListener("click", (e) => {
      stop(e);
      lock = !lock;
      e.currentTarget.classList.toggle("on", lock);
    });
    el.querySelector(".apply").addEventListener("click", (e) => {
      stop(e);
      perform(verb, opt.id, { value: Number(rw.value), value2: Number(rh.value), label: `${rw.value} × ${rh.value}` });
    });
  } else if (opt.editor === "target") {
    const sizeKb = Math.max(60, Math.round(S.files.reduce((n, f) => Math.min(n || Infinity, f.size), 0) / 1024));
    const def = Math.max(30, Math.round(sizeKb * 0.4));
    el.innerHTML = `${head}
      <div class="row"><label>Size</label><input type="range" min="20" max="${sizeKb}" value="${Math.min(def, sizeKb)}"/><output class="kb">${Math.min(def, sizeKb)}</output></div>`;
    const range = el.querySelector("input");
    const out = el.querySelector("output");
    range.addEventListener("input", () => {
      out.textContent = range.value;
      paintAll();
    });
    el.querySelector(".apply").addEventListener("click", (e) => {
      stop(e);
      perform(verb, opt.id, { value: Number(range.value), label: `${opt.title} · ${range.value} KB` });
    });
  } else if (opt.editor === "rename") {
    const full = S.files[0].name;
    const dot = S.files[0].kind === "folder" ? -1 : full.lastIndexOf(".");
    const stem = dot > 0 ? full.slice(0, dot) : full;
    const ext = dot > 0 ? full.slice(dot) : "";
    el.innerHTML = `${head}
      <div class="rename-row"><input type="text" id="rn" value="${escapeHtml(stem)}" spellcheck="false" placeholder="New name"/>${ext ? `<span class="ext">${escapeHtml(ext)}</span>` : ""}</div>
      <div class="actions"><span class="hint">${ext ? "The file type stays the same." : ""}</span></div>`;
    const input = el.querySelector("#rn");
    const go = () => {
      let name = input.value.trim();
      if (!name) return input.focus();
      // The file type is kept automatically, so typing it again doesn't double it.
      if (ext && !name.toLowerCase().endsWith(ext.toLowerCase())) name += ext;
      perform(verb, opt.id, { text: name });
    };
    el.querySelector(".apply").addEventListener("click", (e) => {
      stop(e);
      go();
    });
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") go();
    });
    invoke("set_typing", { on: true }).then(() => {
      setTimeout(() => {
        input.focus();
        input.select();
      }, 80);
    });
  }
  paintAll();
  el.querySelectorAll("input, button").forEach((n) => n.addEventListener("click", stop));
  placeFlyout(verb);
  const scroller = el.closest(".flyout-box") || (S.style === "menu" ? el.parentElement : null);
  if (scroller) {
    const bottom = el.offsetTop + el.offsetHeight + 10;
    if (bottom > scroller.scrollTop + scroller.clientHeight) {
      scroller.scrollTop = bottom - scroller.clientHeight;
    }
  }
}

/* ------------------------------------------------------------------ interaction */
const hoverOpensOptions = () => localStorage.getItem("halo.hoverOpen") !== "false";

// "Hover intent": a wedge only takes over once the pointer has actually stayed on it for a
// moment, so quickly passing over one on the way to the open cards doesn't switch away from
// the wedge you're really heading for. A deliberate hover comfortably clears this; a pass-through doesn't.
let hoverTarget = null;
function hoverVerb(verb) {
  clearTimeout(S.hoverTimer);
  hoverTarget = verb;
  const apply = () => {
    if (DIRECT.has(verb)) {
      highlight(null);
      renderCards(null);
    } else if (S.verb !== verb) {
      highlight(verb);
      renderCards(verb);
    }
  };
  if (!S.verb && !DIRECT.has(verb)) {
    apply();
    return;
  }
  S.hoverTimer = setTimeout(() => {
    if (hoverTarget === verb) apply();
  }, 80);
}
function clearHoverTarget(verb) {
  if (hoverTarget === verb) hoverTarget = null;
}

function clickVerb(verb) {
  clearTimeout(S.hoverTimer);
  if (DIRECT.has(verb)) return perform(verb, "", {});
  if (S.verb === verb) {
    // clicking the open wedge again hides its options
    highlight(null);
    renderCards(null);
  } else {
    highlight(verb);
    renderCards(verb);
  }
}

const BUSY_TEXT = { compress: "Compressing…", convert: "Converting…", preset: "Converting…", resize: "Resizing…", share: "Uploading…", qr: "Making QR…" };

// Results appear as a bottom-right notification, so they survive the wheel closing or being clicked away.
// Several can run at once: each gets its own id, so one finishing (or being dismissed) never touches another.
let opCounter = 0;
const newOpId = () => `op_${Date.now()}_${++opCounter}`;

function notify(notice) {
  const thumb = S.info?.data || null; // read before closing, which clears it
  invoke("show_notice", { notice: { thumb, ...notice } });
  closeWidget();
}
function startBusy(verb, indeterminate = false, opId) {
  const file = S.files.length === 1 ? S.files[0].name : S.files.length ? `${S.files.length} files` : "";
  notify({ kind: "busy", opId, verb, title: BUSY_TEXT[verb], file, total: fmtBytes(totalSize()), indeterminate });
}
const say = (text, tone = "ok", opId) => notify({ kind: "toast", opId, text, tone });

const DONE_TITLE = { compress: "Compression Complete!", convert: "Conversion Complete!", resize: "Resize Complete!", preset: "Conversion Complete!" };

async function perform(verb, option, extra = {}) {
  if(verb==="record"){try{await invoke("open_recorder");closeWidget();}catch(e){say(String(e),"error");}return;}
  if (!S.files.length) return invoke("browse_files", {verb, action:{verb, option, value:extra.value ?? null, extra}});
  const files = S.files.slice();
  const thumb = S.info?.data || null;
  const paths = S.files.map((f) => f.path);
  if (verb === "extract") [verb, option] = ["more", "extract"]; // the wheel's Extract segment
  try {
    const quick = `${verb}:${option}`;
    if (quick === "edit:preview" || quick === "files:open") {
      await invoke("open_path", { path: paths[0] });
      return closeWidget();
    }
    if (quick === "files:copy") {
      await invoke("copy_files", { paths });
      return say("Copied. You can paste it anywhere.");
    }
    if (quick === "files:copy_path") {
      await invoke("copy_text", { text: paths.join("\n") });
      return say("Path copied.");
    }
    if (quick === "files:open_location") {
      await invoke("reveal_path", { path: paths[0] });
      return closeWidget();
    }
  } catch (e) {
    return say(String(e), "error");
  }
  if (verb === "more" && option === "qr") {
    const opId = newOpId();
    startBusy("qr", true, opId);
    try {
      const r = await invoke("make_qr", { path: paths[0] });
      return notify({ kind: "done", opId, title: "QR code ready", detail: r.note, paths: [r.out_path] });
    } catch (e) {
      return say(String(e), "error", opId);
    }
  }
  if (verb === "share") {
    const opId = newOpId();
    startBusy("share", true, opId); // an upload can't report real progress, so the ring just spins
    try {
      const links = await invoke("share_files", { paths });
      return notify({ kind: "link", opId, title: "Uploaded!", detail: `${links[0].provider} · ${links[0].expires}`, urls: links.map((l) => l.url) });
    } catch (e) {
      return say(String(e), "error", opId);
    }
  }
  const media = ["compress", "convert", "resize", "preset"].includes(verb);
  const opId = media ? newOpId() : null;
  const req = { files: paths, verb, option, value: extra.value ?? null, value2: extra.value2 ?? null, text: extra.text ?? null, op_id: opId || "" };
  const pending = media ? { verb, option, value: extra.value, value2: extra.value2, label: extra.label } : null;
  if (media) startBusy(verb, false, opId); // progress shows bottom-right, with the results
  else if (option !== "move_to") closeWidget(); // quick actions get out of the way; the result shows bottom-right

  let outcomes;
  try {
    outcomes = await invoke("run_action", { req });
    finish(verb, option, outcomes, { files, thumb, pending, opId });
  } catch (e) {
    return say(String(e), "error", opId);
  }
}

const SILENT = new Set(["open_with", "properties", "show_new_window"]); // they open a window of their own

function finish(verb, option, outcomes, { files, thumb, pending, opId }) {
  const failed = outcomes.find((o) => !o.ok);
  if (failed) {
    if (failed.message === "Cancelled") {
      return notify({ kind: "dismiss", opId });
    }
    return say(failed.message, "error", opId);
  }
  const media = ["compress", "convert", "resize", "preset"].includes(verb);
  if (!media) return SILENT.has(option) ? closeWidget() : say(outcomes[0]?.message || "Done.");
  if (pending) saveLast(pending);

  const made = outcomes.filter((o) => o.out_path);
  if (!made.length) return say(outcomes[0]?.message || "Nothing to do.", "info", opId);
  const before = outcomes.reduce((n, o) => n + o.before, 0);
  const after = outcomes.reduce((n, o) => n + o.after, 0);
  let detail;
  if (verb === "compress" && after < before) detail = `${fmtBytes(before)} → ${fmtBytes(after)} (${Math.round((1 - after / before) * 100)}% smaller)`;
  else if (made.length === 1) detail = `${made[0].out_path.split("\\").pop()} · ${fmtBytes(made[0].after)}`;
  else detail = `${made.length} files created`;
  if (files.length === 1 && files[0].kind === "folder" && !(verb === "compress" && after < before)) detail = outcomes[0].name;
  const note = outcomes.find((o) => o.out_path && o.message)?.message; // e.g. "Couldn't get it smaller ..."
  if (note) detail = note;
  notify({ kind: "done", opId, title: note ? "Saved a copy" : DONE_TITLE[verb], detail, paths: made.map((m) => m.out_path), thumb });
}
/* ------------------------------------------------------------------ busy / done / toast */
const RING = 2 * Math.PI * 114;


function enterBusy(verb) {
  S.busyVerb = verb;
  S.progress = 0;
  S.bytes = 0;
  $("busy").innerHTML = `
    <div class="ring">
      <svg viewBox="0 0 236 236"><circle class="track" cx="118" cy="118" r="114"/><circle class="bar" id="bar" cx="118" cy="118" r="114" stroke-dasharray="${RING}" stroke-dashoffset="${RING}"/></svg>
      ${thumbHtml()}
      <div class="label">${BUSY_TEXT[verb]}<br><span id="pct">0%</span></div>
    </div>
    <div class="sizes" id="sizes">${fmtBytes(totalSize())}</div>
    <button class="btn pill" id="cancel">Cancel</button>`;
  $("cancel").addEventListener("click", () => {
    invoke("cancel_action");
    $("cancel").textContent = "Cancelling…";
  });
  setMode("busy");
}

function updateProgress({ pct, bytes }) {
  if (S.mode !== "busy") return;
  S.progress = Math.max(S.progress, pct);
  const bar = $("bar");
  if (bar) bar.style.strokeDashoffset = String(RING * (1 - Math.min(S.progress, 1)));
  const p = $("pct");
  if (p) p.textContent = `${Math.round(S.progress * 100)}%`;
  if (bytes) S.bytes = bytes;
  const sizes = $("sizes");
  if (sizes) sizes.textContent = S.bytes ? `${fmtBytes(totalSize())} → ${fmtBytes(S.bytes)}` : fmtBytes(totalSize());
}

function closeWidget() {
  clearTimeout(S.closeTimer);
  invoke("hide_widget");
}

let placementSequence = 0;
let placementTimer = null;
async function publishHitRegions(){const regions=[...app.querySelectorAll('.wheel,.cards,.panel,.busy,.done,.toast')].filter(el=>{const style=getComputedStyle(el);return style.display!=='none'&&style.visibility!=='hidden'&&Number(style.opacity)!==0;}).map(el=>{const r=el.getBoundingClientRect();return {x:r.x,y:r.y,w:r.width,h:r.height,round:el.classList.contains('wheel')&&S.style!=='menu'};}).filter(r=>r.w>1&&r.h>1);await invoke('set_hit_regions',{regions});}
async function fitWidget() {
  if(app.classList.contains("tray-corner")&&app.classList.contains("menu")){const menu=app.querySelector(".wheel");if(menu){const height=menu.getBoundingClientRect().height/.78;app.style.setProperty("--tray-menu-y",`${Math.max(20,554-height-20)}px`);}}
  if (S.mode !== "wheel" || !app.classList.contains("shown")) return;
  const sequence = ++placementSequence;
  const run = S.runId;
  const style = S.style;
  try {
    const result = await invoke("fit_widget", { style, flipped: app.classList.contains("flip") });
    if (result?.dragging) { queueWidgetFit(); return; }
    const flipped = typeof result === "boolean" ? result : result?.flipped;
    if (sequence !== placementSequence || run !== S.runId || style !== S.style || typeof flipped !== "boolean") return;
    app.classList.toggle("flip", flipped);
    placeFlyout(S.verb);
    await publishHitRegions();
  } catch (error) { console.warn("Could not fit Halo to the screen", error); }
}
const queueWidgetFit = () => {
  clearTimeout(placementTimer);
  placementTimer = setTimeout(fitWidget, 100);
};
getCurrentWebviewWindow().onMoved?.(queueWidgetFit);
getCurrentWebviewWindow().onResized?.(queueWidgetFit);

/* ------------------------------------------------------------------ lifecycle */
function reset() {
  S.scan = null;
  S.drop = false;
  app.classList.remove("dropzone", "over");
  clearTimeout(S.hoverTimer);
  clearTimeout(S.closeTimer);
  S.runId++;
  S.verb = null;
  S.info = null;
  S.expanded = null;
  S.confirming = null;
  app.classList.remove("shown");
  setMode("wheel");
  $("cards").innerHTML = "";
}

let conversionPresets = [];
invoke("get_presets").then(list=>{conversionPresets=list;}).catch(()=>{});
listen("halo://show", async ({ payload }) => {
  conversionPresets = await invoke("get_presets").catch(() => []);
  reset();
  app.style.setProperty("--user-opacity", `${(Number(localStorage.getItem("halo.opacity")) || 100) / 100}`);
  S.files = payload.files;
  S.scan = S.files.length === 1 ? S.files[0].folder_scan || null : null;
  app.classList.toggle("tray-corner",!!payload.tray_corner);
  S.drop = !!payload.dropzone;
  app.classList.toggle("dropzone", S.drop);
  S.style = payload.style === "menu" ? "menu" : "wheel";
  app.classList.toggle("menu", S.style === "menu");
  app.classList.toggle("flip", payload.flip);
  if (S.style === "menu") renderMenu();
  else renderWheel();
  requestAnimationFrame(() => {app.classList.add("shown");queueWidgetFit();});
  if (S.drop) {
    // The tray wheel stays open until dismissed, so say how for the first few opens.
    // (say() would close the widget, so the toast goes straight to the notice window.)
    let shown = 0;
    try { shown = Number(localStorage.getItem("halo.closeTipShown") || 0); } catch {}
    if (shown < 3 && !closeHintSeen()) {
      try { localStorage.setItem("halo.closeTipShown", String(shown + 1)); } catch {}
      invoke("show_notice", { notice: { kind: "toast", tone: "ok", text: "Halo stays open. Right-click it or press Esc to close.", duration: 8000 } });
    }
  }
  if(payload.requested_verb){highlight(payload.requested_verb);renderCards(payload.requested_verb);}
  // Started from the Explorer right-click menu: run that action straight away.
  if (payload.auto) {
    const action = payload.auto;
    const match = rawVerbList().flatMap(v => options(v).map(o => ({...o, sourceVerb:v})))
      .find(o => (o.verb ?? o.sourceVerb) === action.verb && (o.option ?? o.id) === action.option && !o.disabled);
    if (!match && action.extra) { say("This action does not support the selected file type.", "error"); return; }
    if (action.extra?.editor) {
      highlight(match.sourceVerb); renderCards(match.sourceVerb);
      const card = [...document.querySelectorAll('.card')].find(el => el.dataset.option === match.id);
      if (card) openEditor(match.sourceVerb, match, card);
    } else perform(action.verb, action.option, {value:action.value, ...action.extra});
  }
  const first = S.files[0];
  if (S.files.length === 1 && first.kind === "folder" && !S.scan) {
    // count what is inside, so the folder can offer compress / convert / resize for its media
    invoke("scan_folder", { path: first.path }).then((scan) => {
      if (scan && S.files[0] === first) {
        S.scan = scan;
        if (S.style === "menu") renderMenu();
        else renderWheel();
      }
    });
  }
  if (first && (first.kind === "image" || first.kind === "video")) {
    const info = await invoke("thumbnail", { path: first.path });
    if (info && S.files[0] === first) {
      S.info = info;
      renderHub();
    }
  }
});
listen("halo://hide", reset);
listen("halo://progress", ({ payload }) => updateProgress(payload));

// Clicking the empty glass around the wheel closes it.
document.addEventListener("pointerdown", (e) => {
  const inside = e.target.closest(".wedge, .mi, .menu-panel, .disc, .card, .cards, .ring, .btn, .done, .toast, .sizes");
  if (S.drop) {
    // The drop zone stays open while you fetch files from another window; a click away only tucks the options in.
    if (!inside) collapseOptions();
    return;
  }
  if (!inside) closeWidget();
});

// Options opened by hovering a wedge tuck away again once the pointer leaves the wheel and its option cards.
const WIDGET_AREA = ".wheel, .cards, .menu-panel, .card, .panel, .sizes, .toast";
let optionsLeaveTimer = null;
function collapseOptions() {
  clearTimeout(optionsLeaveTimer);
  const typing = document.activeElement?.matches?.("input, textarea, select");
  if (!S.verb || S.expanded || S.confirming || typing) return;
  highlight(null);
  renderCards(null);
}
document.addEventListener("pointerover", (e) => {
  if (e.target.closest?.(WIDGET_AREA)) clearTimeout(optionsLeaveTimer);
});
const scheduleCollapse = () => {
  if (!hoverOpensOptions() || !S.verb) return;
  clearTimeout(optionsLeaveTimer);
  optionsLeaveTimer = setTimeout(collapseOptions, 450);
};
document.addEventListener("pointerout", (e) => {
  if (e.relatedTarget?.closest?.(WIDGET_AREA)) return;
  scheduleCollapse();
});
// Leaving the window altogether may not report a pointerout, so listen for it directly too.
document.documentElement.addEventListener("pointerleave", scheduleCollapse);
document.documentElement.addEventListener("mouseleave", scheduleCollapse);

// Holding and moving the pointer over the wheel (including the hub) drags the widget;
// a plain click/tap still reaches its normal handler (e.g. the hub's click-to-browse).
let dragFrom = null;
document.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  if (!e.target.closest(".wheel, .menu-panel")) return;
  if (e.target.closest(".wedge, .mi, .card, .cards, .btn, .done, .toast, .sizes")) return;
  dragFrom = { x: e.clientX, y: e.clientY };
});
document.addEventListener("pointermove", (e) => {
  if (!dragFrom) return;
  if (Math.hypot(e.clientX - dragFrom.x, e.clientY - dragFrom.y) < 4) return;
  dragFrom = null;
  getCurrentWebviewWindow()
    .startDragging()
    .catch((err) => say(String(err), "error"));
});
document.addEventListener("pointerup", () => (dragFrom = null));

// Right-click anywhere closes the widget.
document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  try { localStorage.setItem("halo.closeTipDone", "1"); } catch {}
  closeWidget();
});


/* ------------------------------------------------------------------ drag to choose
   Holding a file and then moving starts a real Explorer drag. The window is a drop target, so the pointer
   keeps working: hovering lights up wedges and cards, and letting go over one picks it. */
let hot = null;
let leaveTimer = null;
const pickAt = (pos) => {
  const k = window.devicePixelRatio || 1;
  return document.elementFromPoint(pos.x / k, pos.y / k)?.closest(".wedge, .mi, .card");
};
function setHot(el) {
  if (el === hot) return;
  hot?.classList.remove("hot");
  hot = el;
  hot?.classList.add("hot");
  if (el?.dataset?.verb) hoverVerb(el.dataset.verb);
}
getCurrentWebviewWindow().onDragDropEvent(({ payload }) => {
  (window.__ddlog = window.__ddlog || []).push(payload.type + (payload.paths ? ":" + payload.paths.length : "")); // test aid: lets automated tests see what arrived
  if (S.mode !== "wheel") return;
  if (payload.type === "enter" || payload.type === "over") {
    clearTimeout(leaveTimer);
    if (S.drop) app.classList.add("over");
    setHot(pickAt(payload.position));
  } else if (payload.type === "drop") {
    // Dropped on an action: choose it. Dropped anywhere else (or on the empty drop zone): these are the files to work on.
    const el = S.drop ? null : pickAt(payload.position);
    setHot(null);
    app.classList.remove("over");
    if (el && !el.classList.contains("editor")) el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    else if (payload.paths?.length) acceptFiles(payload.paths);
  } else {
    setHot(null);
    app.classList.remove("over");
    // The drag left the widget: unless it is the drop zone, the file is being dragged elsewhere, so tidy it away.
    if (!S.drop) leaveTimer = setTimeout(closeWidget, 700);
  }
});
/* ------------------------------------------------------------------ menu style
   The same actions as the wheel, laid out like a context menu with fly-out lists. */
function renderMenu() {
  const SUB = new Set(["compress", "convert", "resize", "edit", "files", "more"]);
  const verbs = verbList();
  const top = verbs.filter((v) => ["compress", "convert", "resize", "extract"].includes(v));
  const rest = verbs.filter((v) => !top.includes(v));
  const order = top.length && rest.length ? [...top, "|", ...rest] : verbs;
  const rows = order
    .map((v) =>
      v === "|"
        ? '<div class="sep"></div>'
        : `<div class="mi" data-verb="${v}"><span class="mi-ico" style="${toneStyle(TONE_BY_VERB[v])}">${svg(VERBS[v].icon)}</span><span class="mi-label">${VERBS[v].label}</span>${SUB.has(v) ? svg("chev", 'class="chev"') : ""}</div>`,
    )
    .join("");
  $("wheel").innerHTML = `<div class="menu-panel"><div class="hub" id="hub"></div>${rows}</div>`;
  renderHub();
  $("wheel").querySelectorAll(".mi").forEach((el) => {
    const verb = el.dataset.verb;
    el.addEventListener("pointerenter", () => {
      if (hoverOpensOptions()) hoverVerb(verb);
    });
    el.addEventListener("click", () => clickVerb(verb));
  });
}

// Lines the fly-out up with the row that opened it, keeping it inside the window.
function placeFlyout(verb) {
  const holder = $("cards");
  if (S.style !== "menu") {
    holder.style.removeProperty("top");
    return;
  }
  const item = verb ? document.querySelector(`.mi[data-verb="${verb}"]`) : null;
  if (!item || !holder.children.length) return;
  const cy = parseFloat(getComputedStyle(app).getPropertyValue("--cy")) || 36;
  const top = cy + item.offsetTop - 8;
  // leave room under the list for its shadow, so the shadow is not cut off by the window edge
  holder.style.top = `${Math.max(14, Math.min(top, app.clientHeight - holder.offsetHeight - 46))}px`;
}
/* ------------------------------------------------------------------ drop zone
   Opened from the tray or by launching Halo: drag files onto the widget, then choose what to do with them. */
async function acceptFiles(paths) {
  const infos = await invoke("file_infos", { paths });
  if (!infos.length) return;
  S.files = infos;
  S.drop = false;
  S.info = null;
  S.scan = infos.length === 1 ? infos[0].folder_scan || null : null;
  S.verb = null;
  app.classList.remove("dropzone", "over");
  $("cards").innerHTML = "";
  if (S.style === "menu") renderMenu();
  else renderWheel();
  const first = infos[0];
  if (infos.length === 1 && first.kind === "folder" && !S.scan) {
    invoke("scan_folder", { path: first.path }).then((scan) => {
      if (scan && S.files[0] === first) {
        S.scan = scan;
        if (S.style === "menu") renderMenu();
        else renderWheel();
      }
    });
  }
  if (first.kind === "image" || first.kind === "video") {
    invoke("thumbnail", { path: first.path }).then((info) => {
      if (info && S.files[0] === first) {
        S.info = info;
        renderHub();
      }
    });
  }
}
