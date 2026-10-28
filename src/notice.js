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

let busyTotal = "";
let timer = null;
let hovering = false;
let pinned = false; // once the pointer has been on the card it stays until you close it
let hideAfter = 0;

function arm(ms) {
  clearTimeout(timer);
  if (!ms) return; // the progress card stays until it is replaced
  timer = setTimeout(() => !hovering && !pinned && dismiss(), ms);
}

function dismiss() {
  clearTimeout(timer);
  host.firstElementChild?.classList.add("leaving");
  setTimeout(() => invoke("hide_notice"), 180);
}

function render(n) {
  hovering = false;
  pinned = false;
  if (n.kind === "done") {
    const thumb = n.thumb ? `<div class="thumb" style="background-image:url('${n.thumb}')"></div>` : `<div class="thumb glyph-only">${DOC_GLYPH}</div>`;
    host.innerHTML = `
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
    document.getElementById("x").onclick = dismiss;
    document.getElementById("open").onclick = () => invoke("open_path", { path: first }).then(dismiss, () => dismiss());
    document.getElementById("show").onclick = () => invoke("reveal_path", { path: first }).then(dismiss);
    document.getElementById("copy").onclick = async (e) => {
      const button = e.currentTarget;
      await invoke("copy_text", { text: n.paths.join("\n") });
      button.lastChild.textContent = "Copied";
      arm(1200);
    };
    document.getElementById("share").onclick = async (e) => {
      const b = e.currentTarget;
      b.disabled = true;
      b.lastChild.textContent = "Uploading…";
      clearTimeout(timer);
      try {
        const links = await invoke("share_files", { paths: n.paths });
        render({ kind: "link", title: "Uploaded!", detail: `${links[0].provider} · ${links[0].expires}`, urls: links.map((l) => l.url), thumb: n.thumb });
      } catch (err) {
        render({ kind: "toast", tone: "error", text: String(err) });
      }
    };
    hideAfter = 6000;
  } else if (n.kind === "busy") {
    const thumb = n.thumb ? `<div class="thumb" style="background-image:url('${n.thumb}')"></div>` : `<div class="thumb glyph-only">${DOC_GLYPH}</div>`;
    host.innerHTML = `
      <div class="busy ${n.indeterminate ? "indeterminate" : ""}">
        <div class="progress-head">
          ${thumb}<div class="label">${esc(n.title)}<span>Working on your files</span></div>
          <span id="pct">${n.indeterminate ? "" : "0%"}</span>
        </div>
        <div class="progress-track" role="progressbar" aria-label="${esc(n.title)}" aria-valuemin="0" aria-valuemax="100" ${n.indeterminate ? "" : 'aria-valuenow="0"'}><div class="progress-fill" id="bar"></div></div>
        <div class="progress-footer"><div class="sizes" id="sizes">${esc(n.total || "")}</div>
          <button class="btn pill" id="cancel">Cancel</button></div>
      </div>`;
    busyTotal = n.total || "";
    document.getElementById("cancel").onclick = (e) => {
      invoke("cancel_action");
      e.currentTarget.textContent = "Cancelling…";
      e.currentTarget.disabled = true;
    };
    hideAfter = 0;
  } else if (n.kind === "link") {
    const thumb = n.thumb ? `<div class="thumb" style="background-image:url('${n.thumb}')"></div>` : `<div class="thumb glyph-only">${DOC_GLYPH}</div>`;
    host.innerHTML = `
      <div class="done">
        <div class="badge">${svg("check")}</div>
        <div class="close" id="x">${svg("x")}</div>
        <div class="head">${thumb}<div style="min-width:0"><h3>${esc(n.title)}</h3><div class="detail" title="${esc(n.detail)}">${esc(n.detail)}</div></div></div>
        <div class="urlbox" title="${esc(n.urls.join("\n"))}">${esc(n.urls[0])}${n.urls.length > 1 ? ` (+${n.urls.length - 1})` : ""}</div>
        <button class="btn primary" id="copylink">${svg("link")}Copy Link</button>
        <button class="btn" id="openlink">${svg("file")}Open Link</button>
      </div>`;
    document.getElementById("x").onclick = dismiss;
    document.getElementById("copylink").onclick = async (e) => {
      const button = e.currentTarget;
      await invoke("copy_text", { text: n.urls.join("\n") });
      button.lastChild.textContent = "Copied";
      arm(1500);
    };
    document.getElementById("openlink").onclick = () => invoke("open_path", { path: n.urls[0] }).then(dismiss, () => dismiss());
    hideAfter = 8000;
  } else if (n.kind === "update") {
    host.innerHTML = `
      <div class="done">
        <div class="badge info">${svg("info")}</div>
        <div class="close" id="x">${svg("x")}</div>
        <div class="head"><div style="min-width:0"><h3>Halo ${esc(n.version)} is ready</h3><div class="detail">A new version has been downloaded. Restart to finish updating.</div></div></div>
        <button class="btn primary" id="restart">${svg("check")}Restart &amp; update</button>
        <button class="btn" id="later">Later</button>
      </div>`;
    document.getElementById("x").onclick = dismiss;
    document.getElementById("later").onclick = dismiss;
    document.getElementById("restart").onclick = async (e) => {
      const button = e.currentTarget;
      button.disabled = true;
      button.lastChild.textContent = "Updating…";
      clearTimeout(timer);
      pinned = true;
      try {
        await invoke("install_update");
      } catch (err) {
        button.disabled = false;
        button.lastChild.textContent = "Restart & update";
        const detail = host.querySelector(".detail");
        if (detail) detail.textContent = String(err).includes("Finish your") ? String(err) : "Could not install the update. Try again from Settings.";
      }
    };
    hideAfter = 20000;
  } else {
    // Quick messages use the same card as results: a badge on top, then the message.
    const error = n.tone === "error";
    const info = n.tone === "info";
    host.innerHTML = `
      <div class="done msg">
        <div class="badge ${error ? "error" : info ? "info" : ""}">${svg(error ? "bang" : info ? "info" : "check")}</div>
        <div class="close" id="x">${svg("x")}</div>
        <h3>${esc(n.text)}</h3>
      </div>`;
    document.getElementById("x").onclick = dismiss;
    hideAfter = n.duration || (error ? 4500 : 2200);
  }
  arm(hideAfter);
}

// Stays put while the pointer is on it; leaves a moment after the pointer moves away.
host.addEventListener("pointerenter", () => {
  hovering = true;
  pinned = true;
  clearTimeout(timer);
});
host.addEventListener("pointerleave", () => {
  hovering = false; // pinned cards stay; unpinned ones carry on counting down
});

listen("halo://notice", ({ payload }) => render(payload));

// Live progress for the notification card.
listen("halo://progress", ({ payload }) => {
  const bar = document.getElementById("bar");
  if (!bar || host.querySelector(".busy.indeterminate")) return;
  const progress = Math.max(0, Math.min(payload.pct, 1));
  bar.style.transform = `scaleX(${progress})`;
  bar.parentElement.setAttribute("aria-valuenow", String(Math.round(progress * 100)));
  const pct = document.getElementById("pct");
  if (pct) pct.textContent = `${Math.round(progress * 100)}%`;
  const sizes = document.getElementById("sizes");
  if (sizes && payload.bytes) sizes.textContent = `${busyTotal} → ${fmtBytes(payload.bytes)}`;
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
