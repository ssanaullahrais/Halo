import "@fontsource-variable/inter";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const host = document.getElementById("n");

const ICON = {
  check: '<path d="m5 12.5 4.5 4.5L19 7"/>',
  x: '<path d="M6 6l12 12M18 6 6 18"/>',
  bang: '<path d="M12 6v8"/><path d="M12 18v.5"/>',
  info: '<path d="M12 11v6"/><path d="M12 7v.5"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  open: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  link: '<path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1"/><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1"/>',
};
const svg = (name) => `<svg viewBox="0 0 24 24" aria-hidden="true">${ICON[name]}</svg>`;
const DOC_GLYPH =
  '<svg viewBox="0 0 64 64"><path d="M14 4h26l12 12v40a4 4 0 0 1-4 4H14a4 4 0 0 1-4-4V8a4 4 0 0 1 4-4z" fill="#dfe8f7"/><path d="M40 4v12h12z" fill="#9db4da"/><path d="M20 30h24M20 38h24M20 46h16" stroke="#7b93bd" stroke-width="3" stroke-linecap="round"/></svg>';

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

// Every card (one per running/finished operation, or a one-off message) is tracked here by id, so
// several can be shown stacked at once: starting or dismissing one never touches another. A card
// without an id of its own (a plain toast) gets a throwaway one so it still gets its own slot.
let anonCounter = 0;
const cards = new Map(); // id -> { kind, el, timer, hovering, pinned, busyTotal }

function resize() {
  // Tell the backend how tall the stack actually is so the window hugs its content: a single
  // card doesn't drag along empty, click-eating space, and several don't get clipped.
  requestAnimationFrame(() => invoke("resize_notice", { height: Math.ceil(host.getBoundingClientRect().height) || 1 }));
}

function removeCard(id) {
  const card = cards.get(id);
  if (!card) return;
  clearTimeout(card.timer);
  cards.delete(id);
  card.el.classList.add("leaving");
  setTimeout(() => {
    card.el.remove();
    if (cards.size === 0) invoke("hide_notice");
    resize();
  }, 180);
}

function arm(id, ms) {
  const card = cards.get(id);
  if (!card) return;
  clearTimeout(card.timer);
  if (!ms) return; // the progress card stays until it is replaced
  card.timer = setTimeout(() => !card.hovering && !card.pinned && removeCard(id), ms);
}

function cardShell(id) {
  let card = cards.get(id);
  if (card) return card;
  const el = document.createElement("div");
  el.className = "card";
  el.dataset.id = id;
  host.appendChild(el); // newest nearest the corner, like Windows' own stack
  card = { el, timer: null, hovering: false, pinned: false, busyTotal: "" };
  cards.set(id, card);
  el.addEventListener("pointerenter", () => {
    card.hovering = true;
    card.pinned = true;
    clearTimeout(card.timer);
  });
  el.addEventListener("pointerleave", () => {
    card.hovering = false; // pinned cards stay; unpinned ones carry on counting down
  });
  return card;
}

function render(n) {
  const id = n.opId || `anon_${++anonCounter}`;
  const card = cardShell(id);
  card.hovering = false;
  card.pinned = false;
  const el = card.el;
  let hideAfter = 0;

  if (n.kind === "dismiss") {
    removeCard(id);
    return;
  } else if (n.kind === "done") {
    const thumb = n.thumb ? `<div class="thumb" style="background-image:url('${n.thumb}')"></div>` : `<div class="thumb glyph-only">${DOC_GLYPH}</div>`;
    el.innerHTML = `
      <div class="done">
        <div class="badge">${svg("check")}</div>
        <div class="close" id="x">${svg("x")}</div>
        <div class="head">${thumb}<div style="min-width:0"><h3>${esc(n.title)}</h3><div class="detail" title="${esc(n.detail)}">${esc(n.detail)}</div></div></div>
        <button class="btn primary" id="open">${svg("file")}Open File</button>
        <button class="btn" id="show">${svg("open")}Show in Folder</button>
        <button class="btn" id="copy">${svg("link")}Copy Path</button>
        <button class="btn" id="share">${svg("link")}Upload & get link</button>
      </div>`;
    const first = n.paths[0];
    el.querySelector("#x").onclick = () => removeCard(id);
    el.querySelector("#open").onclick = () => invoke("open_path", { path: first }).then(() => removeCard(id), () => removeCard(id));
    el.querySelector("#show").onclick = () => invoke("reveal_path", { path: first }).then(() => removeCard(id));
    el.querySelector("#copy").onclick = async (e) => {
      const button = e.currentTarget;
      await invoke("copy_text", { text: n.paths.join("\n") });
      button.lastChild.textContent = "Copied";
      arm(id, 1200);
    };
    el.querySelector("#share").onclick = async (e) => {
      const b = e.currentTarget;
      b.disabled = true;
      b.lastChild.textContent = "Uploading…";
      clearTimeout(card.timer);
      try {
        const links = await invoke("share_files", { paths: n.paths });
        render({ kind: "link", opId: id, title: "Uploaded!", detail: `${links[0].provider} · ${links[0].expires}`, urls: links.map((l) => l.url), thumb: n.thumb });
      } catch (err) {
        render({ kind: "toast", opId: id, tone: "error", text: String(err) });
      }
    };
    hideAfter = 6000;
  } else if (n.kind === "busy") {
    const initial = Math.max(0, Math.min(1, Number(n.pct) || 0));
    const thumb = n.thumb ? `<div class="thumb" style="background-image:url('${n.thumb}')"></div>` : `<div class="thumb glyph-only">${DOC_GLYPH}</div>`;
    el.innerHTML = `
      <div class="busy ${n.indeterminate ? "indeterminate" : ""}">
        <div class="progress-head">
          ${thumb}<div class="label">${esc(n.title)}<span title="${esc(n.file || "")}">${esc(n.file || "Working on your files")}</span></div>
          <span id="pct">${n.indeterminate ? "" : `${Math.round(initial * 100)}%`}</span>
        </div>
        <div class="progress-track" role="progressbar" aria-label="${esc(n.title)}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(initial * 100)}"><div class="progress-fill" id="bar" style="transform:scaleX(${initial})"></div></div>
        <div class="progress-footer"><div class="sizes" id="sizes">${esc(n.total || "")}</div>
          <button class="btn pill" id="cancel">Cancel</button></div>
      </div>`;
    card.busyTotal = n.total || "";
    el.querySelector("#cancel").onclick = (e) => {
      invoke("cancel_action", { opId: id });
      e.currentTarget.textContent = "Cancelling…";
      e.currentTarget.disabled = true;
    };
    hideAfter = 0;
  } else if (n.kind === "link") {
    const thumb = n.thumb ? `<div class="thumb" style="background-image:url('${n.thumb}')"></div>` : `<div class="thumb glyph-only">${DOC_GLYPH}</div>`;
    el.innerHTML = `
      <div class="done">
        <div class="badge">${svg("check")}</div>
        <div class="close" id="x">${svg("x")}</div>
        <div class="head">${thumb}<div style="min-width:0"><h3>${esc(n.title)}</h3><div class="detail" title="${esc(n.detail)}">${esc(n.detail)}</div></div></div>
        <div class="urlbox" title="${esc(n.urls.join("\n"))}">${esc(n.urls[0])}${n.urls.length > 1 ? ` (+${n.urls.length - 1})` : ""}</div>
        <button class="btn primary" id="copylink">${svg("link")}Copy Link</button>
        <button class="btn" id="openlink">${svg("file")}Open Link</button>
      </div>`;
    el.querySelector("#x").onclick = () => removeCard(id);
    el.querySelector("#copylink").onclick = async (e) => {
      const button = e.currentTarget;
      await invoke("copy_text", { text: n.urls.join("\n") });
      button.lastChild.textContent = "Copied";
      arm(id, 1500);
    };
    el.querySelector("#openlink").onclick = () => invoke("open_path", { path: n.urls[0] }).then(() => removeCard(id), () => removeCard(id));
    hideAfter = 8000;
  } else if (n.kind === "update") {
    el.innerHTML = `
      <div class="done">
        <div class="badge info">${svg("info")}</div>
        <div class="close" id="x">${svg("x")}</div>
        <div class="head"><div style="min-width:0"><h3>Halo ${esc(n.version)} is ready</h3><div class="detail">It'll install automatically once nothing's running, or update now.</div></div></div>
        <button class="btn primary" id="restart">${svg("check")}Restart &amp; update now</button>
        <button class="btn" id="later">Later</button>
      </div>`;
    el.querySelector("#x").onclick = () => removeCard(id);
    el.querySelector("#later").onclick = () => removeCard(id);
    el.querySelector("#restart").onclick = async (e) => {
      const button = e.currentTarget;
      button.disabled = true;
      button.lastChild.textContent = "Updating…";
      clearTimeout(card.timer);
      card.pinned = true;
      try {
        await invoke("install_update");
      } catch (err) {
        button.disabled = false;
        button.lastChild.textContent = "Restart & update";
        const detail = el.querySelector(".detail");
        if (detail) detail.textContent = String(err).includes("Finish your") ? String(err) : "Could not install the update. Try again from Settings.";
      }
    };
    hideAfter = 20000;
  } else {
    // Quick messages use the same card as results: a badge on top, then the message.
    const error = n.tone === "error";
    const info = n.tone === "info";
    el.innerHTML = `
      <div class="done msg">
        <div class="badge ${error ? "error" : info ? "info" : ""}">${svg(error ? "bang" : info ? "info" : "check")}</div>
        <div class="close" id="x">${svg("x")}</div>
        <h3>${esc(n.text)}</h3>
      </div>`;
    el.querySelector("#x").onclick = () => removeCard(id);
    hideAfter = n.duration || (error ? 4500 : 2200);
  }
  arm(id, hideAfter);
  resize();
}

listen("halo://notice", ({ payload }) => render(payload));
listen("halo://operation-finished", ({ payload }) => { if (cards.has(payload.opId)) render(payload); });
listen("halo://operation-state", ({ payload: n }) => {
  const card = cards.get(n.opId), busy = card?.el.querySelector('.busy');
  if (!busy) return;
  busy.classList.toggle('indeterminate', !!n.indeterminate);
  const label = busy.querySelector('.label');
  if (label?.firstChild) label.firstChild.textContent = n.title;
  const progress = Math.max(0, Math.min(1, Number(n.pct) || 0));
  busy.querySelector('#pct').textContent = n.indeterminate ? '' : `${Math.round(progress * 100)}%`;
  const bar = busy.querySelector('#bar');bar.style.transform = `scaleX(${progress})`;
  bar.parentElement.setAttribute('aria-valuenow', String(Math.round(progress * 100)));
  if (n.cancelling) { const button = busy.querySelector('#cancel');button.textContent = 'Cancelling…';button.disabled = true; }
});

// Live progress for the notification card it belongs to.
listen("halo://progress", ({ payload }) => {
  const card = cards.get(payload.opId);
  if (!card) return;
  const bar = card.el.querySelector("#bar");
  if (!bar || card.el.querySelector(".busy.indeterminate")) return;
  const progress = Math.max(0, Math.min(payload.pct, 1));
  bar.style.transform = `scaleX(${progress})`;
  bar.parentElement.setAttribute("aria-valuenow", String(Math.round(progress * 100)));
  const pct = card.el.querySelector("#pct");
  if (pct) pct.textContent = `${Math.round(progress * 100)}%`;
  const sizes = card.el.querySelector("#sizes");
  if (sizes && payload.bytes) sizes.textContent = `${card.busyTotal} → ${fmtBytes(payload.bytes)}`;
});

function fmtBytes(n) {
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 100 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}
