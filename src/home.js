import "@fontsource-variable/inter";
import './update-ui.js';
import {createElement,Clapperboard,Globe,FileText,Wifi,ImagePlus,ChevronDown,Check,Info,RefreshCw,Sun,CloudUpload,Monitor,MousePointer2,Orbit,File,CircleDot,Menu,Grid2X2,Shield,PanelTop,MousePointerClick,Bell,Pencil,RotateCcw} from "lucide";
import {listen} from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import img402Icon from "./assets/providers/img402.ico";
import catboxIcon from "./assets/providers/catbox.ico";
import tmpfilesIcon from "./assets/providers/tmpfiles.ico";

const $ = (id) => document.getElementById(id);
const SETTINGS_ICONS={Sun,CloudUpload,Monitor,Info,MousePointer2,Orbit,File,CircleDot,Menu,Grid2X2,Shield,PanelTop,MousePointerClick,Bell,Pencil,RefreshCw,RotateCcw,Check};
for(const host of document.querySelectorAll('[data-settings-icon]'))host.append(createElement(SETTINGS_ICONS[host.dataset.settingsIcon],{'aria-hidden':'true','stroke-width':1.8}));

// Each service's own icon; Uguu publishes none, so it gets a simple cloud tile.
const PROVIDER_LOGO = {
  img402: `<img src="${img402Icon}" alt="" />`,
  catbox: `<img src="${catboxIcon}" alt="" />`,
  tmpfiles: `<img src="${tmpfilesIcon}" alt="" />`,
  uguu: createElement(CloudUpload, {'aria-hidden':'true'}).outerHTML,
};

/* ---------------------------------------------------------------- settings: saved vs. being edited */
const DEFAULTS = {
  hold_ms: 250,
  startup: false,
  auto_updates: true,
  shortcut: "ctrl+alt+KeyH",
  context_menu: false,
  native_context_menu: false,
  style: "wheel",
  share: [],
  notifications: true,
  ctrl_click: true,
  tray_on_close: true,
  save_mode: "beside",
  // These two are front-end-only wheel preferences (not round-tripped through the Rust
  // settings store), but still live in `draft`/`saved` so Save behaves consistently for them.
  hover_open: true,
  opacity: 100,
};
let saved = null;
let draft = null;
let providers = [];
let recording = false;
let justSaved = false;

const clone = (o) => JSON.parse(JSON.stringify(o));
const isDirty = () => saved && JSON.stringify(draft) !== JSON.stringify(saved);

function say(text, bad = false) {
  const m = $("msg");
  m.textContent = text;
  m.classList.toggle("bad", bad);
}

/* ---------------------------------------------------------------- shortcut keys */
const MOD_NAMES = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", super: "Win" };
const prettyKey = (code) =>
  code
    .replace(/^Key/, "")
    .replace(/^Digit/, "")
    .replace(/^Arrow/, "")
    .replace(/^Numpad/, "Num ")
    .replace("Backquote", "`")
    .replace("Minus", "-")
    .replace("Equal", "=")
    .replace("BracketLeft", "[")
    .replace("BracketRight", "]")
    .replace("Semicolon", ";")
    .replace("Quote", "'")
    .replace("Comma", ",")
    .replace("Period", ".")
    .replace("Slash", "/")
    .replace("Backslash", "\\");
const KEY_OK = /^(Key[A-Z]|Digit\d|F\d{1,2}|Space|Enter|Tab|Backquote|Minus|Equal|Bracket(Left|Right)|Semicolon|Quote|Comma|Period|Slash|Backslash|Arrow(Up|Down|Left|Right)|Home|End|PageUp|PageDown|Insert|Delete|Numpad.+)$/;

function renderKeys() {
  const el = $("keys");
  if (recording) {
    el.innerHTML = '<span class="hint">Press your keys…</span>';
    $("changeKeys").lastChild.textContent = "Cancel";
    return;
  }
  $("changeKeys").lastChild.textContent = "Change shortcut";
  el.innerHTML = draft.shortcut
    .split("+")
    .map((p) => `<kbd>${MOD_NAMES[p] || prettyKey(p)}</kbd>`)
    .join("<em>+</em>");
}

$("changeKeys").addEventListener("click", () => {
  recording = !recording;
  say("");
  renderKeys();
  if (recording) $("changeKeys").focus();
});
$("changeKeys").addEventListener("blur", () => {
  if (recording) {
    recording = false;
    renderKeys();
  }
});
$("changeKeys").addEventListener("keydown", (e) => {
  if (!recording) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") {
    recording = false;
    return renderKeys();
  }
  if (["Control", "Alt", "Shift", "Meta"].includes(e.key) || !KEY_OK.test(e.code)) return;
  if (!(e.ctrlKey || e.altKey || e.metaKey)) return say("Please include Ctrl, Alt or Win so it doesn't clash with normal typing.", true);
  const mods = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "super"].filter(Boolean);
  draft.shortcut = [...mods, e.code].join("+");
  recording = false;
  refresh();
});

/* ---------------------------------------------------------------- drawing the page from `draft` */
function paintRange() {
  const r = $("hold");
  r.style.setProperty("--p", `${((r.value - r.min) / (r.max - r.min)) * 100}%`);
  $("holdOut").textContent = `${(Number(r.value) / 1000).toFixed(2)} s`;
}

// The three "How it works" lines follow the current settings.
function paintSteps() {
  const secs = (draft.hold_ms / 1000).toFixed(2).replace(/0$/, "");
  $("step1").textContent = `Hold any file for ${secs} s${draft.ctrl_click ? ", or Ctrl + click it" : ""}.`;
  $("step2").textContent = `Pick compress, convert, resize and more from the ${draft.style === "menu" ? "menu" : "wheel"}.`;
  $("step3").textContent = draft.save_mode === "ask" ? "A new file is saved where you choose." : "A new file is saved beside the original.";
}

// The button itself confirms the save: "Save settings" while there is something to save, "Saved" once it is.
function paintSave() {
  const dirty = isDirty();
  $("save").disabled = !dirty;
  $("save").querySelector(":scope > span:not(.settings-icon)").textContent = !dirty && justSaved ? "Saved" : "Save settings";
  $("save").classList.toggle("done", !dirty && justSaved);
}

function refresh() {
  paintSteps();
  $("hold").value = draft.hold_ms;
  paintRange();
  $("startup").checked = draft.startup;
  $("autoUpdates").checked = draft.auto_updates;
  $("tray").checked = draft.tray_on_close;
  $("ctx").checked = draft.context_menu;
  $("ctxHint").textContent = draft.native_context_menu ? "Adds Halo to the main Windows 11 right-click menu." : 'Adds Halo under "Show more options" in Explorer.';
  $("notify").checked = draft.notifications;
  $("ctrlclick").checked = draft.ctrl_click;
  $("ctrlKeys").classList.toggle("off", !draft.ctrl_click);
  $("hoverOpen").checked = draft.hover_open;
  $("opacity").value = draft.opacity;
  paintOpacity();
  document.querySelectorAll("#style button").forEach((b) => b.classList.toggle("on", b.dataset.style === draft.style));
  document.querySelectorAll('#saveMode input[name="save"]').forEach((r) => (r.checked = r.value === draft.save_mode));
  renderKeys();
  renderProviders();
  paintSave();
}

/* controls */
$("hold").addEventListener("input", () => {
  draft.hold_ms = Number($("hold").value);
  paintRange();
  paintSteps();
  paintSave();
});
function paintOpacity() {
  const r = $("opacity");
  r.style.setProperty("--p", `${((r.value - r.min) / (r.max - r.min)) * 100}%`);
  $("opacityOut").textContent = `${r.value}%`;
}
// These two are front-end-only (not part of the Rust settings round-trip), so they apply to
// the wheel at once via localStorage, but still flow through `draft` so Save behaves normally.
$("opacity").addEventListener("input", () => {
  draft.opacity = Number($("opacity").value);
  localStorage.setItem("halo.opacity", draft.opacity);
  paintOpacity();
  paintSave();
});
$("hoverOpen").addEventListener("change", () => {
  draft.hover_open = $("hoverOpen").checked;
  localStorage.setItem("halo.hoverOpen", draft.hover_open ? "true" : "false");
  say("");
  refresh();
});
for (const [id, key] of [["autoUpdates", "auto_updates"], ["startup", "startup"], ["tray", "tray_on_close"], ["ctx", "context_menu"], ["notify", "notifications"], ["ctrlclick", "ctrl_click"]]) {
  $(id).addEventListener("change", () => {
    draft[key] = $(id).checked;
    say("");
    refresh();
  });
}
$("style").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  draft.style = b.dataset.style;
  say("");
  refresh();
});
$("saveMode").addEventListener("change", (e) => {
  if (e.target.name === "save") {
    draft.save_mode = e.target.value;
    say("");
    refresh();
  }
});

/* ---------------------------------------------------------------- save / restore */
$("save").addEventListener("click", async () => {
  $("save").disabled = true;
  try {
    await invoke("set_settings", { settings: draft });
    saved = { ...clone(draft), ...await invoke("get_settings") };
    draft = clone(saved);
    justSaved = true;
    say("");
  } catch (e) {
    say(String(e), true);
  }
  refresh();
});
$("restore").addEventListener("click", () => {
  draft = { ...clone(DEFAULTS), share: providers.map((p) => p.id) };
  say("");
  refresh();
});

/* ---------------------------------------------------------------- upload services */
function renderProviders() {
  $("providers").innerHTML = providers
    .map(
      (p) => `<div class="row">
        <span class="logo-tile ${p.id}">${PROVIDER_LOGO[p.id] || ""}</span>
        <div class="label"><strong>${p.name}</strong><span>${p.note}</span></div>
        <label class="switch"><input type="checkbox" data-id="${p.id}" ${draft.share.includes(p.id) ? "checked" : ""} /><i></i></label>
      </div>`,
    )
    .join("");
}
$("providers").addEventListener("change", (e) => {
  const box = e.target.closest("input[data-id]");
  if (!box) return;
  draft.share = box.checked ? [...new Set([...draft.share, box.dataset.id])] : draft.share.filter((x) => x !== box.dataset.id);
  say("");
  refresh();
});

/* ---------------------------------------------------------------- system check */
const installing = new Set();
function renderChecks(list) {
  const esc = value => String(value).replace(/[&<>"\']/g, c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","\'":"&#39;"}[c]));
  const icon = node => createElement(node, {'aria-hidden':'true'}).outerHTML;
  const icons={ffmpeg:Clapperboard,edge:Globe,pdfium:FileText,internet:Wifi,imagemagick:ImagePlus};
  const expanded = new Set([...$("checks").querySelectorAll('details[open]')].map(el=>el.dataset.check));
  $("checks").innerHTML = list.map(c=>{
    const busy=installing.has(c.id)||c.installing;
    const button=c.fix?`<button class="fix" data-fix="${c.id}" ${busy?'disabled':''}>${busy?'Installing…':esc(c.fix)}</button>`:'';
    return `<details class="check ${c.ok?'':'bad'}" data-check="${c.id}" ${expanded.has(c.id)?'open':''}><summary><span class="check-icon check-icon-${c.id}">${icon(icons[c.id]||Info)}</span><div class="label"><strong>${esc(c.name)}</strong><span>${esc(c.purpose)}</span><span class="state">${icon(c.ok?Check:Info)}${busy?'Installing…':c.ok?'Ready':'Needs setup'} · ${esc(c.detail)}</span></div><span class="check-badge"><i></i>${busy?'Installing…':c.ok?'Ready':'Needs setup'}</span><span class="check-chevron">${icon(ChevronDown)}</span></summary><div class="check-details"><span>${esc(c.detail)}</span>${button}</div></details>`;
  }).join('');
  const missing = list.filter((c) => !c.ok && !["internet","imagemagick"].includes(c.id)).length;
  const settingUp=list.some(c=>c.installing);
  $("systemSummary").textContent=settingUp?'Installing missing components…':missing?'Some required components need setup.':'All required components are ready.';
  $("systemSummaryDetail").textContent=settingUp?'Setup runs automatically in the background.':missing?'Missing components install automatically. Expand an item for details or retry.':'Halo is ready to run on your system.';
  const pill = $("status");
  pill.classList.toggle("warn-pill", missing > 0);
  const words = missing > 0 ? `${missing} ${missing === 1 ? "item needs" : "items need"} setup. Click to see.` : "Running in the tray";
  pill.querySelector("b").textContent = words;
  pill.title = words;
}
let requirementsRequest = null,requirementsPending=false;
listen("halo://requirements-changed",()=>{if(requirementsRequest)requirementsPending=true;else refreshChecks();}).catch(console.error);
function refreshChecks() {
  if (requirementsRequest) return requirementsRequest;
  const button = $("recheck"), label = $("recheckLabel");
  button.disabled = true;
  button.classList.add("is-checking");
  button.setAttribute("aria-busy", "true");
  label.textContent = "Checking…";
  $("systemSummary").textContent = "Checking your system…";
  $("systemSummaryDetail").textContent = "Checking the components Halo uses.";
  requirementsRequest = (async () => {
    try {
      const checks = await invoke("get_requirements");
      if (!Array.isArray(checks) || !checks.length) throw new Error("No system check results were returned.");
      renderChecks(checks);
      const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
      $("systemSummaryDetail").textContent += ` Last checked at ${time}.`;
    } catch (err) {
      $("systemSummary").textContent = "Could not check your system.";
      $("systemSummaryDetail").textContent = "Click Check again to retry.";
      say(String(err), true);
    } finally {
      button.disabled = false;
      button.classList.remove("is-checking");
      button.removeAttribute("aria-busy");
      label.textContent = "Check again";
      requirementsRequest = null;
      if(requirementsPending){requirementsPending=false;setTimeout(refreshChecks,0);}
    }
  })();
  return requirementsRequest;
}
$("checks").addEventListener("click", async (e) => {
  const b = e.target.closest("button[data-fix]");
  if (!b) return;
  const id = b.dataset.fix;
  if (installing.has(id)) return;
  installing.add(id);
  b.disabled = true;
  b.textContent = "Installing…";
  say("Installing. This can take a few minutes. Approve any Windows permission prompt.");
  try {
    const result = await invoke("install_requirement", { id });
    say(String(result || "Installed."));
  } catch (err) {
    say(String(err), true);
  }
  installing.delete(id);
  await refreshChecks();
});
$("recheck").addEventListener("click", () => {
  say("");
  refreshChecks();
});
$("status").addEventListener("click", () => $("status").classList.contains("warn-pill") && showPage("system"));

/* ---------------------------------------------------------------- navigation */
function showPage(name) {
  document.querySelectorAll("#nav button").forEach((b) => b.classList.toggle("on", b.dataset.page === name));
  document.querySelectorAll(".page").forEach((p) => {
    p.hidden = p.dataset.page !== name;
    if (!p.hidden) {
      $("title").textContent = p.dataset.title;
      $("subtitle").textContent = p.dataset.sub;
    }
  });
  $("pages").scrollTop = 0;
}
$("nav").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (b) showPage(b.dataset.page);
});

/* ---------------------------------------------------------------- about */
invoke("app_info").then((info) => {
  $("ver").textContent = `v${info.version}`;
  $("ver2").textContent = `v${info.version}`;
  $("settingsPath").textContent = info.settings_file || "Not saved yet";
  $("showSettings").onclick = () => info.settings_file && invoke("reveal_path", { path: info.settings_file }).catch(() => {});
});

/* ---------------------------------------------------------------- start */
async function start() {
  providers = await invoke("list_providers");
  const s = await invoke("get_settings");
  saved = {
    ...clone(DEFAULTS),
    ...s,
    share: s.share || providers.map((p) => p.id),
    hover_open: localStorage.getItem("halo.hoverOpen") !== "false",
    opacity: Number(localStorage.getItem("halo.opacity")) || 100,
  };
  draft = clone(saved);
  refresh();
  refreshChecks();
}
start();



