"use strict";

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const THEMES = [
  { id: "nacht", name: "Nacht", c: ["#0f0f17", "#8b5cf6", "#14c8b8"] },
  { id: "mitternacht", name: "Mitternacht", c: ["#000", "#a78bfa", "#22d3ee"] },
  { id: "ozean", name: "Ozean", c: ["#0b1320", "#3b82f6", "#06b6d4"] },
  { id: "wald", name: "Wald", c: ["#0c140f", "#22c55e", "#a3e635"] },
  { id: "glut", name: "Glut", c: ["#140d0b", "#f97316", "#ef4444"] },
  { id: "rose", name: "Rosé", c: ["#140c12", "#ec4899", "#f59e0b"] },
  { id: "hell", name: "Hell", c: ["#f4f4f8", "#7c3aed", "#0d9488"] },
];

const S = {
  meta: null,
  settings: null,
  apps: [],
  selfUpdate: null,
  filter: "all",
  query: "",
  progress: {},     // repo -> {phase, done, total, speed, message}
  details: {},      // repo -> Details
  current: null,    // Repo im Popup
  loading: false,
  firstLoad: true,
  queue: Promise.resolve(),
  refreshTimer: null,
  cancelled: new Set(),
};

// ---------- Hilfen ----------
function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function fmtBytes(n) {
  if (!n) return "0 B";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(n >= 100 || i === 0 ? 0 : 1)} ${u[i]}`;
}
const rtf = new Intl.RelativeTimeFormat("de", { numeric: "auto" });
function fmtAgo(dateish) {
  if (!dateish) return "";
  const t = typeof dateish === "number" ? dateish * 1000 : Date.parse(dateish);
  if (!t) return "";
  const s = (t - Date.now()) / 1000;
  const a = Math.abs(s);
  if (a < 60) return "gerade eben";
  if (a < 3600) return rtf.format(Math.round(s / 60), "minute");
  if (a < 86400) return rtf.format(Math.round(s / 3600), "hour");
  if (a < 86400 * 30) return rtf.format(Math.round(s / 86400), "day");
  if (a < 86400 * 365) return rtf.format(Math.round(s / (86400 * 30)), "month");
  return rtf.format(Math.round(s / (86400 * 365)), "year");
}
function fmtDate(dateish) {
  const t = typeof dateish === "number" ? dateish * 1000 : Date.parse(dateish);
  return t ? new Date(t).toLocaleDateString("de-DE", { day: "2-digit", month: "2-digit", year: "numeric" }) : "–";
}
function prettyName(repo) {
  return repo.replace(/[-_]+/g, " ").replace(/\b(pc|appimage)\b/gi, (m) => m.toUpperCase());
}
function hue(str) {
  let h = 0;
  for (const c of str) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}
function iconHtml(app) {
  if (app.icon) return `<div class="app-icon"><img src="${app.icon}" alt="" /></div>`;
  const h = hue(app.repo);
  const letter = (app.repo.replace(/^lak[uü]en/i, "") || app.repo)[0]?.toUpperCase() || "?";
  return `<div class="app-icon" style="background:linear-gradient(135deg,hsl(${h} 65% 52%),hsl(${(h + 50) % 360} 70% 42%))">${esc(letter)}</div>`;
}
function toast(msg, kind = "", ms = 4500) {
  const el = document.createElement("div");
  el.className = `toast ${kind}`;
  el.textContent = msg;
  $("#toasts").appendChild(el);
  setTimeout(() => el.remove(), ms);
}
function errText(e) {
  return typeof e === "string" ? e : e?.message || JSON.stringify(e);
}

// Einfaches, sicheres Markdown für Release-Notizen
function md(src) {
  if (!src || !src.trim()) return `<p class="hint">Keine Notizen zu diesem Release.</p>`;
  const lines = src.replace(/\r/g, "").split("\n");
  let out = "", inList = false, inCode = false;
  const inline = (s) =>
    esc(s)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
      .replace(/(^|\W)\*([^*]+)\*(?=\W|$)/g, "$1<i>$2</i>")
      .replace(/\[([^\]]+)\]\((https:\/\/[^)\s]+)\)/g, (m, t, u) => `<a href="#" data-href="${u}">${t}</a>`)
      .replace(/(^|[\s(])(https:\/\/[^\s<)]+)/g, (m, p, u) => `${p}<a href="#" data-href="${u}">${u}</a>`);
  for (const raw of lines) {
    const l = raw.trimEnd();
    if (l.trim().startsWith("```")) {
      if (inList) { out += "</ul>"; inList = false; }
      out += inCode ? "</code></pre>" : "<pre><code>";
      inCode = !inCode;
      continue;
    }
    if (inCode) { out += esc(raw) + "\n"; continue; }
    const li = l.match(/^\s*[-*+]\s+(.*)/);
    if (li) {
      if (!inList) { out += "<ul>"; inList = true; }
      out += `<li>${inline(li[1])}</li>`;
      continue;
    }
    if (inList) { out += "</ul>"; inList = false; }
    const h = l.match(/^(#{1,4})\s+(.*)/);
    if (h) { out += `<h${h[1].length + 1}>${inline(h[2])}</h${h[1].length + 1}>`; continue; }
    if (!l.trim()) continue;
    out += `<p>${inline(l)}</p>`;
  }
  if (inList) out += "</ul>";
  if (inCode) out += "</code></pre>";
  return out;
}

// ---------- Darstellung ----------
function visibleApps() {
  const st = S.settings;
  const q = S.query.trim().toLowerCase();
  let list = S.apps.filter((a) => {
    if (st.hidden.includes(a.repo)) return false;
    if (!st.show_forks && a.fork && !a.installed) return false;
    if (!st.show_archived && a.archived && !a.installed) return false;
    if (q) {
      const hay = `${a.repo} ${prettyName(a.repo)} ${a.description} ${(a.topics || []).join(" ")} ${a.language || ""}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
  const counts = {
    all: list.length,
    installed: list.filter((a) => a.installed).length,
    updates: list.filter((a) => a.status === "update").length,
    available: list.filter((a) => a.status === "install").length,
    favorites: list.filter((a) => st.favorites.includes(a.repo)).length,
    norelease: list.filter((a) => a.status === "no_release" || a.status === "no_asset").length,
  };
  const f = S.filter;
  list = list.filter((a) =>
    f === "all" ? true :
    f === "installed" ? !!a.installed :
    f === "updates" ? a.status === "update" :
    f === "available" ? a.status === "install" :
    f === "favorites" ? st.favorites.includes(a.repo) :
    f === "norelease" ? a.status === "no_release" || a.status === "no_asset" : true
  );
  const rank = { update: 0, ok: 1, install: 2, no_asset: 3, no_release: 4 };
  const fav = (a) => (st.favorites.includes(a.repo) ? 0 : 1);
  const byName = (a, b) => a.repo.localeCompare(b.repo, "de", { sensitivity: "base" });
  const sorters = {
    status: (a, b) => fav(a) - fav(b) || rank[a.status] - rank[b.status] || byName(a, b),
    name: byName,
    updated: (a, b) => (Date.parse(b.release?.published_at || b.pushed_at || 0) || 0) - (Date.parse(a.release?.published_at || a.pushed_at || 0) || 0),
    launched: (a, b) => (b.installed?.last_launch || 0) - (a.installed?.last_launch || 0) || byName(a, b),
  };
  list.sort(sorters[st.sort] || sorters.status);
  return { list, counts };
}

function actionFor(app) {
  const p = S.progress[app.repo];
  if (app.busy || (p && !["done", "error"].includes(p.phase))) return { label: "Abbrechen", cls: "danger", act: "cancel" };
  switch (app.status) {
    case "install": return { label: "Installieren", cls: "primary", act: "install" };
    case "update": return { label: "Update", cls: "update", act: "install" };
    case "ok": return app.installed ? { label: app.running ? "Läuft" : "Starten", cls: "run", act: "launch" } : null;
    default: return null;
  }
}

function cardHtml(app) {
  const enabled = app.status !== "no_release";
  const fav = S.settings.favorites.includes(app.repo);
  const act = actionFor(app);
  const rel = app.release;
  let sub = "";
  if (app.status === "no_release") sub = "Noch kein Release";
  else if (app.status === "no_asset") sub = `${rel.tag} · kein Linux-AppImage`;
  else if (app.installed && app.status === "update") sub = `${app.installed.tag} → ${rel.tag}`;
  else if (app.installed) sub = `${app.installed.tag} installiert`;
  else if (rel) sub = `${rel.tag} · ${fmtAgo(rel.published_at)}`;
  const badges = [];
  if (app.status === "update") badges.push(`<span class="badge update">${app.held ? "Zurückgesetzt" : "Update"}</span>`);
  if (app.running) badges.push(`<span class="badge run">Läuft</span>`);
  if (app.fork) badges.push(`<span class="badge">Fork</span>`);
  if (app.archived) badges.push(`<span class="badge">Archiv</span>`);
  if (app.private) badges.push(`<span class="badge">Privat</span>`);
  const p = S.progress[app.repo];
  const showP = p && !["done", "error"].includes(p.phase);
  const pct = showP && p.total ? Math.min(100, (p.done / p.total) * 100) : 0;
  return `
  <div class="card ${enabled ? "clickable" : "disabled"}" tabindex="0" data-repo="${esc(app.repo)}"
       title="${enabled ? "" : "Noch kein Release auf GitHub"}">
    <button class="fav ${fav ? "on" : ""}" data-fav title="Favorit">${fav ? "★" : "☆"}</button>
    <button class="hide-btn" data-hide title="Ausblenden">✕</button>
    <div class="card-top">
      ${iconHtml(app)}
      <div>
        <div class="card-name">${esc(prettyName(app.repo))}</div>
        <div class="card-sub">${esc(sub)}</div>
      </div>
    </div>
    <div class="card-desc">${esc(app.description || (enabled ? "" : "Wird aktiv, sobald ein Release veröffentlicht ist."))}</div>
    <div class="card-bottom">
      ${badges.join("")}
      ${act ? `<button class="btn small ${act.cls} action" data-act="${act.act}" ${app.running && act.act === "launch" ? "" : ""}>${act.label}</button>` : ""}
    </div>
    ${showP ? `<div class="progress ${p.total ? "" : "indeterminate"}"><div class="bar" style="width:${pct}%"></div></div>` : ""}
  </div>`;
}

function render() {
  if (!S.settings) return;
  const { list, counts } = visibleApps();
  for (const [k, v] of Object.entries(counts)) {
    const el = $(`[data-count="${k}"]`);
    if (el) el.textContent = v ? v : "";
  }
  $$(".chip").forEach((c) => c.classList.toggle("active", c.dataset.filter === S.filter));
  const grid = $("#grid");
  const focused = document.activeElement?.closest?.(".card")?.dataset.repo;
  grid.classList.toggle("list", S.settings.view === "list");
  grid.innerHTML = list.map(cardHtml).join("");
  if (focused) $(`.card[data-repo="${CSS.escape(focused)}"]`)?.focus({ preventScroll: true });
  const updates = S.apps.filter((a) => a.status === "update" && !S.settings.hidden.includes(a.repo));
  $("#updateAllBtn").classList.toggle("hidden", updates.length < 1);
  $("#updateAllBtn").textContent = updates.length > 1 ? `Alle aktualisieren (${updates.length})` : "Aktualisieren";
  const empty = $("#empty");
  if (!list.length && !S.loading) {
    empty.classList.remove("hidden");
    empty.innerHTML = S.apps.length
      ? `Nichts gefunden${S.query ? ` für „${esc(S.query)}“` : ""}.`
      : `Noch keine Repos gefunden. Prüfe die Verbindung und drück <kbd>F5</kbd>.`;
  } else empty.classList.add("hidden");
  if (S.current) renderModal();
}

function skeleton() {
  $("#grid").innerHTML = Array.from({ length: 8 }, () => `<div class="card skeleton"></div>`).join("");
}

function setStatus(res) {
  const inst = S.apps.filter((a) => a.installed).length;
  const left = [`${S.apps.length} Repos`, `${inst} installiert`];
  if (res?.offline) left.push("offline – zeige letzten Stand");
  $("#statusLeft").textContent = left.join(" · ");
  const right = [];
  if (res && res.rate_remaining >= 0) {
    right.push(`GitHub-Abfragen übrig: ${res.rate_remaining}`);
  }
  right.push(`Stand: ${new Date().toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" })}`);
  $("#statusRight").textContent = right.join(" · ");
  const n = $("#notice");
  let msg = "";
  if (res?.error) msg = res.error;
  else if (res?.rate_limited) {
    const at = res.rate_reset ? new Date(res.rate_reset * 1000).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" }) : "später";
    msg = `GitHub-Abfragelimit erreicht – zeige gespeicherten Stand. Wieder frei um ${at}. Mit einem Token in den Einstellungen passiert das nicht mehr.`;
  } else if (res?.offline) msg = "Keine Verbindung zu GitHub – du siehst den zuletzt geladenen Stand. Installierte Apps lassen sich trotzdem starten.";
  n.textContent = msg;
  n.classList.toggle("hidden", !msg);
}

// ---------- Laden ----------
async function refresh({ silent = false } = {}) {
  if (S.loading) return;
  S.loading = true;
  $("#refreshBtn").classList.add("spin");
  if (!S.apps.length) skeleton();
  try {
    const res = await invoke("list_apps");
    S.apps = res.apps;
    S.selfUpdate = res.self_update;
    setStatus(res);
    renderSelfBanner();
  } catch (e) {
    if (!silent) toast(errText(e), "err");
  } finally {
    S.loading = false;
    $("#refreshBtn").classList.remove("spin");
    render();
  }
}

async function pollRunning() {
  try {
    const running = await invoke("running_apps");
    let changed = false;
    for (const a of S.apps) {
      const r = running.includes(a.repo);
      if (a.running !== r) { a.running = r; changed = true; }
    }
    if (changed) render();
  } catch {}
}

function scheduleRefresh() {
  clearInterval(S.refreshTimer);
  const m = Number(S.settings.refresh_minutes) || 0;
  if (m > 0) S.refreshTimer = setInterval(() => refresh({ silent: true }).then(autoUpdate), m * 60000);
}

// ---------- Aktionen ----------
function findApp(repo) { return S.apps.find((a) => a.repo === repo); }

function install(repo, { quiet = false } = {}) {
  const app = findApp(repo);
  if (!app) return Promise.resolve(false);
  const wasUpdate = app.status === "update";
  app.busy = true;
  S.progress[repo] = { phase: "prepare", done: 0, total: 0, speed: 0, message: "Warte …" };
  render();
  // Nacheinander, damit die Leitung nicht geteilt wird
  S.cancelled.delete(repo);
  const job = S.queue.then(async () => {
    if (S.cancelled.has(repo)) {
      S.cancelled.delete(repo);
      delete S.progress[repo];
      app.busy = false;
      render();
      return false;
    }
    try {
      S.cancelled.delete(repo);
      await invoke("install_app", { repo });
      if (!quiet) toast(`${prettyName(repo)} ${wasUpdate ? "aktualisiert" : "installiert"} ✓`, "ok");
      return true;
    } catch (e) {
      const t = errText(e);
      if (t !== "Abgebrochen") toast(`${prettyName(repo)}: ${t}`, "err", 7000);
      else toast(`${prettyName(repo)}: Download abgebrochen`, "warn");
      return false;
    } finally {
      delete S.progress[repo];
      await refresh({ silent: true });
    }
  });
  S.queue = job.catch(() => {});
  return job;
}

async function launch(repo) {
  const app = findApp(repo);
  if (app?.running) {
    toast(`${prettyName(repo)} läuft bereits`);
    return;
  }
  try {
    await invoke("launch_app", { repo });
    toast(`${prettyName(repo)} wird gestartet …`, "ok", 2500);
    if (app) { app.running = true; render(); }
    setTimeout(pollRunning, 1500);
  } catch (e) {
    toast(errText(e), "err");
  }
}

async function cancel(repo) {
  S.cancelled.add(repo);
  await invoke("cancel_download", { repo });
}

async function updateAll() {
  const list = S.apps.filter((a) => a.status === "update" && !S.settings.hidden.includes(a.repo));
  if (!list.length) return;
  toast(`Aktualisiere ${list.length} App${list.length > 1 ? "s" : ""} …`);
  const results = await Promise.all(list.map((a) => install(a.repo, { quiet: true })));
  const ok = results.filter(Boolean).length;
  toast(`${ok} von ${list.length} aktualisiert`, ok === list.length ? "ok" : "warn");
}

async function autoUpdate() {
  if (S.settings.auto_update_apps) {
    const list = S.apps.filter((a) => a.status === "update" && !a.held && !a.busy && !S.settings.hidden.includes(a.repo));
    if (list.length) {
      toast(`Automatisches Update: ${list.map((a) => prettyName(a.repo)).join(", ")}`);
      await Promise.all(list.map((a) => install(a.repo, { quiet: true })));
      toast("Automatische Updates fertig", "ok");
    }
  }
  if (S.settings.auto_update_self && S.selfUpdate && S.meta.is_appimage) selfUpdate();
}

function confirmBox(title, text, yes = "OK") {
  return new Promise((resolve) => {
    $("#cTitle").textContent = title;
    $("#cText").textContent = text;
    $("#cYes").textContent = yes;
    openOverlay("#confirmModal");
    const done = (v) => { closeOverlay("#confirmModal"); $("#cYes").onclick = $("#cNo").onclick = null; resolve(v); };
    $("#cYes").onclick = () => done(true);
    $("#cNo").onclick = () => done(false);
    $("#cYes").focus();
  });
}

async function saveSettings(patch = {}) {
  Object.assign(S.settings, patch);
  try {
    await invoke("save_settings", { settings: S.settings });
  } catch (e) {
    toast(`Einstellungen nicht gespeichert: ${errText(e)}`, "err");
  }
}

function toggleFav(repo) {
  const f = new Set(S.settings.favorites);
  f.has(repo) ? f.delete(repo) : f.add(repo);
  saveSettings({ favorites: [...f] });
  render();
}

function hideRepo(repo) {
  if (!S.settings.hidden.includes(repo)) saveSettings({ hidden: [...S.settings.hidden, repo] });
  toast(`${prettyName(repo)} ausgeblendet – in den Einstellungen wieder einblendbar`);
  closeOverlay("#appModal");
  render();
}

// ---------- Selbst-Update ----------
function renderSelfBanner() {
  const b = $("#selfBanner");
  const u = S.selfUpdate;
  if (!u) { b.classList.add("hidden"); return; }
  b.classList.remove("hidden");
  $("#selfBannerTitle").textContent = `LakuenSuite ${u.tag} ist da (du hast ${S.meta.version})`;
  if (!S.meta.is_appimage) {
    $("#selfBannerSub").textContent = "Diese Version läuft nicht als AppImage – das Update bitte von GitHub laden.";
    $("#selfUpdateBtn").textContent = "Auf GitHub öffnen";
  } else {
    $("#selfBannerSub").textContent = `Lädt ${fmtBytes(u.size)}, prüft die Datei und startet neu. Einstellungen und Apps bleiben erhalten.`;
    $("#selfUpdateBtn").textContent = "Jetzt aktualisieren";
  }
}

async function selfUpdate() {
  if (!S.meta.is_appimage) { invoke("open_external", { target: S.selfUpdate.url }); return; }
  $("#selfUpdateBtn").disabled = true;
  $("#selfProgress").classList.remove("hidden");
  try {
    await invoke("self_update");
    $("#selfBannerSub").textContent = "Fertig – LakuenSuite startet neu …";
  } catch (e) {
    toast(`Update fehlgeschlagen: ${errText(e)}`, "err", 8000);
    $("#selfUpdateBtn").disabled = false;
    $("#selfProgress").classList.add("hidden");
  }
}

// ---------- Popup ----------
async function openApp(repo) {
  const app = findApp(repo);
  if (!app || app.status === "no_release") return;
  S.current = repo;
  $("#mMenu").classList.add("hidden");
  renderModal();
  openOverlay("#appModal");
  $("#mPrimary").focus();
  if (!S.details[repo]) {
    try {
      S.details[repo] = await invoke("app_details", { repo });
      if (S.current === repo) renderModal();
    } catch {}
  }
}

function renderModal() {
  const app = findApp(S.current);
  if (!app) { closeOverlay("#appModal"); return; }
  const d = S.details[app.repo] || {};
  const rel = app.release;
  $("#mIcon").innerHTML = iconHtml(app);
  $("#mName").textContent = d.name || prettyName(app.repo);
  const badges = [];
  if (rel) badges.push(`<span class="badge">${esc(rel.tag)}</span>`);
  if (rel?.published_at) badges.push(`<span class="badge">veröffentlicht ${esc(fmtAgo(rel.published_at))}</span>`);
  if (rel?.asset) badges.push(`<span class="badge">${fmtBytes(rel.asset.size)}</span>`);
  if (rel?.asset?.has_checksum) badges.push(`<span class="badge ok">mit Prüfsumme</span>`);
  if (d.category) badges.push(`<span class="badge">${esc(d.category)}</span>`);
  if (app.language) badges.push(`<span class="badge">${esc(app.language)}</span>`);
  if (app.status === "update") badges.push(`<span class="badge update">Update verfügbar</span>`);
  if (app.status === "ok" && app.installed) badges.push(`<span class="badge ok">Aktuell</span>`);
  if (app.running) badges.push(`<span class="badge run">Läuft</span>`);
  $("#mBadges").innerHTML = badges.join("");

  const short = d.short || app.description || "";
  $("#mShort").textContent = short || (S.details[app.repo] ? "Keine Beschreibung hinterlegt." : "Lade Beschreibung …");
  const readme = d.readme && d.readme !== short ? d.readme : "";
  $("#mReadme").textContent = readme;
  $("#mReadme").classList.toggle("hidden", !readme);

  const p = S.progress[app.repo];
  const active = p && !["done", "error"].includes(p.phase);
  const prim = $("#mPrimary");
  prim.className = "btn big";
  prim.disabled = false;
  if (active) {
    prim.textContent = { prepare: "Vorbereiten …", download: "Lädt …", verify: "Prüft …", install: "Installiert …" }[p.phase] || "Arbeitet …";
    prim.disabled = true;
    prim.classList.add("primary");
  } else if (app.status === "install") {
    prim.textContent = `Installieren${rel?.asset ? ` (${fmtBytes(rel.asset.size)})` : ""}`;
    prim.classList.add("primary");
  } else if (app.status === "update") {
    prim.textContent = `Update auf ${rel.tag}`;
    prim.classList.add("update");
  } else if (app.status === "ok" && app.installed) {
    prim.textContent = app.running ? "Läuft bereits" : "Starten";
    prim.classList.add("run");
  } else {
    prim.textContent = "Kein Linux-AppImage im Release";
    prim.disabled = true;
  }
  $("#mCancel").classList.toggle("hidden", !active);

  const bar = $("#mProgress");
  bar.classList.toggle("hidden", !active);
  const txt = $("#mProgressText");
  txt.classList.toggle("hidden", !active);
  if (active) {
    const pct = p.total ? Math.min(100, (p.done / p.total) * 100) : 0;
    bar.classList.toggle("indeterminate", !p.total || p.phase !== "download");
    $(".bar", bar).style.width = `${pct}%`;
    if (p.phase === "download" && p.total) {
      const eta = p.speed ? Math.max(0, Math.round((p.total - p.done) / p.speed)) : null;
      txt.textContent = `${fmtBytes(p.done)} von ${fmtBytes(p.total)} · ${fmtBytes(p.speed)}/s${eta !== null ? ` · noch ${eta < 60 ? eta + " s" : Math.round(eta / 60) + " min"}` : ""}`;
    } else txt.textContent = p.message || "";
  }

  // Menü
  const inst = app.installed;
  $('[data-act="folder"]', $("#mMenu")).disabled = !inst;
  $('[data-act="uninstall"]', $("#mMenu")).disabled = !inst || active;
  $('[data-act="reinstall"]', $("#mMenu")).disabled = !inst || !rel?.asset || active;
  const rb = $("#mRollback");
  rb.disabled = !app.has_previous || active;
  rb.textContent = app.has_previous && inst?.prev_tag ? `Zurück auf ${inst.prev_tag}` : "Vorherige Version wiederherstellen";

  // Werte
  const stats = [];
  if (inst) {
    stats.push(["Installiert", `${inst.tag} · ${fmtDate(inst.installed_at)}`]);
    stats.push(["Prüfung", inst.verified ? "SHA-256 bestätigt" : "ohne Prüfsumme"]);
    stats.push(["Gestartet", inst.launches ? `${inst.launches}× · zuletzt ${fmtAgo(inst.last_launch)}` : "noch nie"]);
    stats.push(["Größe", fmtBytes(inst.size)]);
  }
  if (rel?.asset) stats.push(["Downloads", String(rel.asset.downloads)]);
  if (app.stars) stats.push(["Sterne", String(app.stars)]);
  $("#mStats").innerHTML = stats.map(([k, v]) => `<div class="stat"><b>${esc(k)}</b>${esc(v)}</div>`).join("");

  if (rel) {
    $("#mNotesWrap").classList.remove("hidden");
    $("#mNotesTitle").textContent = `Was ist neu in ${rel.name || rel.tag}`;
    $("#mNotes").innerHTML = md(rel.body);
    if (app.status === "update" && !$("#mNotesWrap").dataset.touched) $("#mNotesWrap").open = true;
  } else $("#mNotesWrap").classList.add("hidden");
}

// ---------- Overlays ----------
function openOverlay(sel) { $(sel).classList.remove("hidden"); }
function closeOverlay(sel) {
  $(sel).classList.add("hidden");
  if (sel === "#appModal") {
    const repo = S.current;
    S.current = null;
    $("#mNotesWrap").removeAttribute("data-touched");
    $("#mNotesWrap").open = false;
    if (repo) $(`.card[data-repo="${CSS.escape(repo)}"]`)?.focus({ preventScroll: true });
  }
}
function topOverlay() {
  return ["#confirmModal", "#notesModal", "#tutorial", "#appModal", "#settingsModal"].find((s) => !$(s).classList.contains("hidden"));
}

// ---------- Einstellungen ----------
function applyTheme() {
  document.documentElement.dataset.theme = S.settings.theme || "nacht";
}

function openSettings() {
  const st = S.settings;
  $("#themes").innerHTML = THEMES.map(
    (t) => `<button class="theme ${st.theme === t.id ? "active" : ""}" data-theme-id="${t.id}">
      <div class="sw">${t.c.map((c) => `<i style="background:${c}"></i>`).join("")}</div><div class="tn">${t.name}</div></button>`
  ).join("");
  $("#sCheck").checked = st.check_on_start;
  $("#sAutoApps").checked = st.auto_update_apps;
  $("#sAutoSelf").checked = st.auto_update_self;
  $("#sRefresh").value = String(st.refresh_minutes);
  $("#sDir").value = st.install_dir;
  $("#sDesktop").checked = st.desktop_entries;
  $("#sPrev").checked = st.keep_previous;
  $("#sForks").checked = st.show_forks;
  $("#sArchived").checked = st.show_archived;
  $("#sToken").value = st.github_token;
  $("#sShortcut").classList.toggle("hidden", !S.meta.is_appimage);
  renderHidden();
  $("#aboutText").innerHTML = `LakuenSuite <b>${esc(S.meta.version)}</b> (${esc(S.meta.arch)}) – erstellt von Dominic Lesina x Claude.<br/>
    Daten bleiben lokal, keine Anmeldung, keine Statistik. Quelle der Apps: github.com/${esc(S.meta.owner)}.`;
  openOverlay("#settingsModal");
}

function renderHidden() {
  const h = S.settings.hidden;
  $("#sHidden").innerHTML = h.length
    ? `<span class="hint">Ausgeblendet:</span>` + h.map((r) => `<button class="badge" data-unhide="${esc(r)}" title="Wieder einblenden">${esc(prettyName(r))} ✕</button>`).join("")
    : "";
}

function bindSettings() {
  $("#themes").addEventListener("click", (e) => {
    const b = e.target.closest("[data-theme-id]");
    if (!b) return;
    saveSettings({ theme: b.dataset.themeId });
    applyTheme();
    $$(".theme").forEach((x) => x.classList.toggle("active", x === b));
  });
  const tog = (id, key, after) => $(id).addEventListener("change", (e) => { saveSettings({ [key]: e.target.checked }); after?.(e.target.checked); });
  tog("#sCheck", "check_on_start");
  tog("#sAutoApps", "auto_update_apps");
  tog("#sAutoSelf", "auto_update_self");
  tog("#sPrev", "keep_previous");
  tog("#sForks", "show_forks", render);
  tog("#sArchived", "show_archived", render);
  tog("#sDesktop", "desktop_entries", async (on) => {
    try {
      const n = await invoke("set_desktop_entries", { enabled: on });
      if (n) toast(on ? `${n} Startmenü-Einträge angelegt` : `${n} Startmenü-Einträge entfernt`, "ok");
    } catch (e) { toast(errText(e), "err"); }
  });
  $("#sRefresh").addEventListener("change", (e) => { saveSettings({ refresh_minutes: Number(e.target.value) }); scheduleRefresh(); });
  $("#sDir").addEventListener("change", (e) => {
    saveSettings({ install_dir: e.target.value.trim() || S.meta.default_install_dir });
    toast("Neue Installationen landen im neuen Ordner. Bereits installierte Apps bleiben, wo sie sind.");
  });
  $("#sDirReset").addEventListener("click", () => { $("#sDir").value = S.meta.default_install_dir; saveSettings({ install_dir: S.meta.default_install_dir }); });
  $("#sDirOpen").addEventListener("click", () => invoke("open_install_dir", { repo: null }).catch((e) => toast(errText(e), "err")));
  $("#sToken").addEventListener("change", async (e) => {
    await saveSettings({ github_token: e.target.value.trim() });
    toast(e.target.value.trim() ? "Token gespeichert – lade neu …" : "Token entfernt", "ok");
    refresh();
  });
  $("#sTokenShow").addEventListener("click", () => {
    const i = $("#sToken");
    i.type = i.type === "password" ? "text" : "password";
    $("#sTokenShow").textContent = i.type === "password" ? "Zeigen" : "Verbergen";
  });
  $("#sShortcut").addEventListener("click", async () => {
    try { await invoke("create_suite_shortcut"); toast("LakuenSuite ist jetzt im Startmenü", "ok"); }
    catch (e) { toast(errText(e), "err"); }
  });
  $("#sHidden").addEventListener("click", (e) => {
    const b = e.target.closest("[data-unhide]");
    if (!b) return;
    saveSettings({ hidden: S.settings.hidden.filter((r) => r !== b.dataset.unhide) });
    renderHidden();
    render();
  });
  $("#sTutorial").addEventListener("click", () => { closeOverlay("#settingsModal"); showTutorial(); });
  $("#sRepo").addEventListener("click", () => invoke("open_external", { target: `https://github.com/${S.meta.owner}/${S.meta.self_repo}` }));
}

// ---------- Einführung ----------
let tutStep = 0;
function showTutorial() {
  tutStep = 0;
  updateTut();
  openOverlay("#tutorial");
  $("#tNext").focus();
}
function updateTut() {
  const steps = $$(".tut-step");
  steps.forEach((s, i) => s.classList.toggle("hidden", i !== tutStep));
  $("#tNext").textContent = tutStep === steps.length - 1 ? "Los geht's" : "Weiter";
}
function endTutorial() {
  closeOverlay("#tutorial");
  if (!S.settings.tutorial_done) saveSettings({ tutorial_done: true });
}

// ---------- Tastatur ----------
function moveFocus(dx, dy) {
  const cards = $$(".card");
  if (!cards.length) return;
  const cur = document.activeElement?.closest?.(".card");
  if (!cur) { cards[0].focus(); return; }
  const r = cur.getBoundingClientRect();
  const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
  let best = null, bestD = Infinity;
  for (const c of cards) {
    if (c === cur) continue;
    const b = c.getBoundingClientRect();
    const x = b.left + b.width / 2, y = b.top + b.height / 2;
    const ddx = x - cx, ddy = y - cy;
    if (dx && Math.sign(ddx) !== dx) continue;
    if (dy && Math.sign(ddy) !== dy) continue;
    if (dx && Math.abs(ddy) > r.height / 2) continue;
    const d = Math.abs(ddx) + Math.abs(ddy) * (dx ? 3 : 1);
    if (d < bestD) { bestD = d; best = c; }
  }
  if (best) { best.focus(); best.scrollIntoView({ block: "nearest" }); }
}

function bindKeys() {
  document.addEventListener("keydown", (e) => {
    const top = topOverlay();
    if (e.key === "Escape") {
      if (top) { e.preventDefault(); if (top === "#confirmModal") $("#cNo").click(); else if (top === "#tutorial") endTutorial(); else closeOverlay(top); }
      else if (S.query) { $("#search").value = ""; S.query = ""; render(); }
      return;
    }
    if ((e.ctrlKey && e.key.toLowerCase() === "f") || (e.key === "/" && !top && document.activeElement !== $("#search"))) {
      e.preventDefault();
      if (top) closeOverlay(top);
      $("#search").focus();
      $("#search").select();
      return;
    }
    if (e.key === "F5" || (e.ctrlKey && e.key.toLowerCase() === "r")) {
      e.preventDefault();
      refresh();
      return;
    }
    if (e.ctrlKey && e.key === ",") { e.preventDefault(); openSettings(); return; }
    if (top) return;
    const inSearch = document.activeElement === $("#search");
    if (inSearch && e.key === "ArrowDown") { e.preventDefault(); $$(".card")[0]?.focus(); return; }
    if (inSearch) return;
    const map = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    if (map[e.key] && (document.activeElement?.closest?.(".card") || document.activeElement === document.body)) {
      e.preventDefault();
      moveFocus(...map[e.key]);
      return;
    }
    const card = document.activeElement?.classList?.contains("card") ? document.activeElement : null;
    if (card && (e.key === "Enter" || e.key === " ")) {
      e.preventDefault();
      openApp(card.dataset.repo);
    }
  });
}

// ---------- Ereignisse ----------
function bindUi() {
  $("#search").addEventListener("input", (e) => { S.query = e.target.value; render(); });
  $("#filters").addEventListener("click", (e) => {
    const c = e.target.closest(".chip");
    if (c) { S.filter = c.dataset.filter; render(); }
  });
  $("#sort").addEventListener("change", (e) => { saveSettings({ sort: e.target.value }); render(); });
  $("#viewBtn").addEventListener("click", () => { saveSettings({ view: S.settings.view === "list" ? "grid" : "list" }); render(); });
  $("#refreshBtn").addEventListener("click", () => refresh());
  $("#settingsBtn").addEventListener("click", openSettings);
  $("#versionChip").addEventListener("click", openSettings);
  $("#updateAllBtn").addEventListener("click", updateAll);
  $("#selfUpdateBtn").addEventListener("click", selfUpdate);
  $("#selfNotesBtn").addEventListener("click", () => {
    if (!S.selfUpdate) return;
    $("#nTitle").textContent = `LakuenSuite ${S.selfUpdate.name}`;
    $("#nBody").innerHTML = md(S.selfUpdate.body);
    openOverlay("#notesModal");
  });

  $("#grid").addEventListener("click", (e) => {
    const card = e.target.closest(".card");
    if (!card) return;
    const repo = card.dataset.repo;
    if (e.target.closest("[data-fav]")) { e.stopPropagation(); toggleFav(repo); return; }
    if (e.target.closest("[data-hide]")) { e.stopPropagation(); hideRepo(repo); return; }
    const act = e.target.closest("[data-act]")?.dataset.act;
    if (act === "install") return install(repo);
    if (act === "launch") return launch(repo);
    if (act === "cancel") return cancel(repo);
    openApp(repo);
  });
  $("#grid").addEventListener("dblclick", (e) => {
    const card = e.target.closest(".card");
    if (!card || e.target.closest("button")) return;
    const app = findApp(card.dataset.repo);
    if (app?.status === "ok" && app.installed) { closeOverlay("#appModal"); launch(app.repo); }
  });

  // Popup
  $("#mPrimary").addEventListener("click", () => {
    const app = findApp(S.current);
    if (!app) return;
    if (app.status === "install" || app.status === "update") install(app.repo);
    else if (app.status === "ok" && app.installed) launch(app.repo);
  });
  $("#mCancel").addEventListener("click", () => S.current && cancel(S.current));
  $("#mGithub").addEventListener("click", () => {
    const app = findApp(S.current);
    if (app) invoke("open_external", { target: app.release?.url || app.url });
  });
  $("#mMoreBtn").addEventListener("click", (e) => { e.stopPropagation(); $("#mMenu").classList.toggle("hidden"); });
  document.addEventListener("click", (e) => { if (!e.target.closest(".menu-wrap")) $("#mMenu").classList.add("hidden"); });
  $("#mNotesWrap").addEventListener("toggle", () => { $("#mNotesWrap").dataset.touched = "1"; });
  $("#mMenu").addEventListener("click", async (e) => {
    const act = e.target.closest("[data-act]")?.dataset.act;
    const repo = S.current;
    if (!act || !repo) return;
    $("#mMenu").classList.add("hidden");
    const name = prettyName(repo);
    try {
      if (act === "folder") await invoke("open_install_dir", { repo });
      if (act === "hide") hideRepo(repo);
      if (act === "reinstall") install(repo);
      if (act === "rollback") {
        const tag = await invoke("rollback_app", { repo });
        toast(`${name} auf ${tag} zurückgesetzt. Automatische Updates pausieren bis zur nächsten neuen Version.`, "ok", 6000);
        await refresh({ silent: true });
      }
      if (act === "uninstall") {
        if (await confirmBox(`${name} deinstallieren?`, "Die App-Datei und ihr Startmenü-Eintrag werden gelöscht. Spielstände und Einstellungen der App selbst bleiben erhalten.", "Deinstallieren")) {
          await invoke("uninstall_app", { repo });
          toast(`${name} deinstalliert`, "ok");
          await refresh({ silent: true });
        }
      }
    } catch (err) { toast(errText(err), "err"); }
  });

  // Overlays schließen
  $$(".overlay").forEach((o) => {
    o.addEventListener("mousedown", (e) => {
      if (e.target === o && o.id !== "confirmModal" && o.id !== "tutorial") closeOverlay("#" + o.id);
    });
    $$("[data-close]", o).forEach((b) => b.addEventListener("click", () => closeOverlay("#" + o.id)));
  });

  // Externe Links
  document.addEventListener("click", (e) => {
    const a = e.target.closest("a[data-href]");
    if (a) { e.preventDefault(); invoke("open_external", { target: a.dataset.href }).catch((err) => toast(errText(err), "err")); }
  });

  // Einführung
  $("#tNext").addEventListener("click", () => {
    if (tutStep >= $$(".tut-step").length - 1) endTutorial();
    else { tutStep++; updateTut(); }
  });
  $("#tSkip").addEventListener("click", endTutorial);

  // Rechtsklick-Menü des Browsers aus (fühlt sich sonst wie eine Webseite an)
  document.addEventListener("contextmenu", (e) => { if (!e.target.closest("input, .md, .m-short, .m-readme")) e.preventDefault(); });
}

// Fortschritt vom Backend
async function bindEvents() {
  await T.event.listen("progress", (ev) => {
    const p = ev.payload;
    if (p.repo === "__self__") {
      const bar = $("#selfProgress .bar");
      if (p.phase === "download" && p.total) bar.style.width = `${(p.done / p.total) * 100}%`;
      $("#selfProgress").classList.toggle("indeterminate", p.phase !== "download");
      if (p.message) $("#selfBannerSub").textContent = p.message;
      else if (p.phase === "download") $("#selfBannerSub").textContent = `${fmtBytes(p.done)} von ${fmtBytes(p.total)} · ${fmtBytes(p.speed)}/s`;
      return;
    }
    S.progress[p.repo] = p;
    const app = findApp(p.repo);
    if (app) app.busy = !["done", "error"].includes(p.phase);
    // Nur die betroffene Kachel + Popup aktualisieren (flüssig)
    const card = $(`.card[data-repo="${CSS.escape(p.repo)}"]`);
    if (card && app) {
      const bar = $(".progress .bar", card);
      if (bar && p.total) bar.style.width = `${Math.min(100, (p.done / p.total) * 100)}%`;
      else if (!bar) render();
    }
    if (S.current === p.repo) renderModal();
  });
}

// ---------- Start ----------
async function main() {
  bindUi();
  bindSettings();
  bindKeys();
  await bindEvents();
  try {
    S.meta = await invoke("get_meta");
  } catch (e) {
    $("#statusLeft").textContent = `Fehler beim Start: ${errText(e)}`;
    return;
  }
  S.settings = S.meta.settings;
  applyTheme();
  $("#versionChip").textContent = S.meta.version;
  $("#sort").value = S.settings.sort;
  if (!S.settings.tutorial_done) showTutorial();
  await refresh();
  if (S.settings.check_on_start) autoUpdate();
  scheduleRefresh();
  setInterval(pollRunning, 5000);
  window.addEventListener("focus", () => pollRunning());
  if (!topOverlay()) $$(".card")[0]?.focus({ preventScroll: true });
}

main();
