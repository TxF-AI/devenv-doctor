import "./styles.css";
import { icon, hydrateIcons } from "./icons.js";
import { t, setLang, getLang, applyStatic } from "./i18n.js";
import { buildPrompt } from "./diag.js";

const { invoke } = window.__TAURI__.core;

const HIVE = { User: "HKCU\\Environment", System: "HKLM\\…\\Session Manager\\Environment" };
const DEFAULT_PROFILE_NAMES = ["Mon environnement", "My environment"];

let report = null;
let isAdmin = false;
let plans = { orphans: null, duplicates: null, compress: null };
let lengths = null;
let conflicts = new Map(); // "Scope:index" → { total, names[] }
let lastMoved = null; // { scope, path } : ligne à faire clignoter après un déplacement
let sync = { kind: "idle", time: null }; // pied de page : état gardé en données pour être retraduit
const filters = { q: "", scope: "all", state: "all" };

const $ = (sel) => document.querySelector(sel);

// Toujours textContent : les chemins viennent du Registre, jamais d'HTML injecté.
function el(tagName, cls, text) {
  const n = document.createElement(tagName);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}
const isAdminError = (err) => String(err).startsWith("AdminRequired");
const scopeSel = () => $("#scope-select").value;
const scopeName = (s) => t(`scope_${s}`);
const fmtTime = (d = new Date()) => d.toLocaleTimeString(t("locale"));
const fmtDate = (d) => d.toLocaleString(t("locale"));
/** Raison d'une sauvegarde : identifiant stable stocké dans le fichier, traduit à l'affichage. */
function reasonLabel(r) {
  if (r.startsWith("avant-profil:")) return t("reason_profile", { name: r.slice("avant-profil:".length) });
  const k = `reason_${r}`;
  const v = t(k);
  return v === k ? r : v;
}

// ─────────────── Données & filtres ───────────────

/** Ordre de résolution Windows : Système puis Utilisateur. */
function allEntries() {
  const tag = (list) => list.map((e, idx) => ({ ...e, idx, scopeLen: list.length }));
  return [...tag(report.system.entries), ...tag(report.user.entries)].map((e, i) => ({ ...e, n: i + 1 }));
}

const conflictOf = (e) => conflicts.get(`${e.scope}:${e.idx}`);
/** Numéro (#) affiché d'une entrée désignée par (scope, index dans le scope). */
const numberOf = (scope, index) => (scope === "System" ? 0 : report.system.entries.length) + index + 1;

function matches(e) {
  if (filters.scope !== "all" && e.scope !== filters.scope) return false;
  if (filters.state === "ok" && !e.exists) return false;
  if (filters.state === "orphan" && e.exists) return false;
  if (filters.state === "dup" && !e.is_duplicate) return false;
  if (filters.state === "conflict" && !conflictOf(e)) return false;
  const q = filters.q.trim().toLowerCase();
  return !q || e.raw_path.toLowerCase().includes(q) || e.expanded_path.toLowerCase().includes(q);
}

// ─────────────── Rendu : stats, tableau, pied de page ───────────────

function badge(text, cls, iconName) {
  const b = el("span", `badge ${cls}`);
  if (iconName) b.append(icon(iconName, 10));
  b.append(text);
  return b;
}

function renderStats(all) {
  const c = [
    [t("st_total"), all.length, "all", "var(--info)"],
    [t("st_valid"), all.filter((e) => e.exists).length, "ok", "var(--ok)"],
    [t("st_orphans"), all.filter((e) => !e.exists).length, "orphan", "var(--bad)"],
    [t("st_dups"), all.filter((e) => e.is_duplicate).length, "dup", "var(--warn)"],
    [t("st_conflicts"), all.filter((e) => conflictOf(e)).length, "conflict", "var(--warn)"],
  ];
  $("#stats").replaceChildren(
    ...c.map(([k, v, state, color]) => {
      const b = el("button", "stat");
      b.style.setProperty("--c", color);
      b.setAttribute("aria-pressed", String(filters.state === state));
      b.title = t("st_filter_title", { k });
      b.append(el("span", "k", k), el("span", "v", String(v)));
      b.addEventListener("click", () => setFilter("state", filters.state === state ? "all" : state));
      return b;
    }),
  );
}

function row(e) {
  const tr = el("tr", (e.exists ? "" : "orphan ") + (e.is_duplicate ? "dup" : ""));
  tr.append(el("td", "ind"), el("td", "n", String(e.n)));
  const st = el("td", "st");
  st.append(e.exists ? badge(t("b_valid"), "ok") : badge(t("b_orphan"), "bad", "circle-alert"));
  if (e.is_duplicate) st.append(badge(t("b_dup"), "warn", "copy"));
  const bs = el("div", "bs");
  while (st.firstChild) bs.append(st.firstChild);
  const cf = conflictOf(e);
  if (cf) {
    const b = badge(t("b_conflict"), "conf", "arrow-left-right");
    b.title = conflictTitle(cf);
    bs.append(b);
  }
  st.append(bs);
  const sc = el("td", "sc");
  sc.append(badge(e.scope === "System" ? "HKLM" : "HKCU", "scope"));
  const p = el("td", "path");
  p.append(el("div", "raw", e.raw_path));
  if (e.expanded_path !== e.raw_path) p.append(el("div", "exp", e.expanded_path));
  if (cf) p.append(el("div", "cf", conflictLine(cf)));
  const act = el("td", "act");
  act.append(moveButtons(e));
  tr.append(st, sc, p, act);
  if (lastMoved && lastMoved.scope === e.scope && lastMoved.path === e.raw_path) {
    tr.classList.add("flash");
    lastMoved = null;
  }
  return tr;
}

/** « node.exe (masqué par #14) · python.exe (gagne) » : 4 noms au plus, le reste en infobulle. */
function conflictLine(cf) {
  const part = (n) => (n.wins ? t("cf_wins", { name: n.name }) : t("cf_masked", { name: n.name, n: numberOf(n.other_scope, n.other_index) }));
  const shown = cf.names.slice(0, 4).map(part).join(" · ");
  return cf.total > 4 ? `${shown} · +${cf.total - 4}` : shown;
}
const conflictTitle = (cf) =>
  cf.names
    .map((n) => (n.wins ? t("cf_title_wins", { name: n.name }) : t("cf_title_masked", { name: n.name, n: numberOf(n.other_scope, n.other_index) })))
    .join("\n");

function moveButtons(e) {
  const box = el("span", "mv");
  const needAdmin = e.scope === "System" && !isAdmin;
  const mk = (dir, ic, label, disabled) => {
    const b = el("button", "");
    b.append(icon(ic, 13));
    b.title = needAdmin ? t("need_admin_title") : label;
    b.setAttribute("aria-label", `${label} : ${e.raw_path}`);
    b.disabled = disabled || needAdmin;
    b.addEventListener("click", () => onMove(e, dir));
    return b;
  };
  box.append(
    mk("up", "chevron-up", t("mv_up"), e.idx === 0),
    mk("down", "chevron-down", t("mv_down"), e.idx === e.scopeLen - 1),
  );
  return box;
}

function renderTable() {
  const all = allEntries();
  const shown = all.filter(matches);
  $("#rows").replaceChildren(...shown.map(row));
  $("#empty").classList.toggle("hidden", shown.length > 0);
  $("#count-txt").textContent = t("count_txt", { shown: shown.length, total: all.length });
  renderStats(all);
  renderLens();
  $("#q-clear").classList.toggle("hidden", filters.q === "");
  for (const [id, key] of [["#chips-state", "state"], ["#chips-scope", "scope"]]) {
    $(id).querySelectorAll(".chip").forEach((c) => c.setAttribute("aria-pressed", String(c.dataset.v === filters[key])));
  }
}

function setFilter(key, value) {
  filters[key] = value;
  if (report) renderTable();
}

// ─────────────── Longueur du PATH ───────────────

const METER_MAX = 8192;
const METER_TICKS = [1024, 2048]; // 8192 = fin de la jauge
const RISK_COLOR = { ok: "var(--ok)", near: "var(--warn)", medium: "var(--warn)", high: "var(--bad)", critical: "var(--bad)" };
const RISK_CLASS = { ok: "", near: "warn", medium: "warn", high: "bad", critical: "bad" };
const fmtNum = (n) => n.toLocaleString(t("locale"));

function renderLens() {
  const box = $("#lens");
  if (!lengths) {
    box.replaceChildren();
    return;
  }
  const cells = [
    [t("lens_hkcu"), lengths.user, t("lens_hkcu_tip")],
    [t("lens_hklm"), lengths.system, t("lens_hklm_tip")],
    [t("lens_eff"), lengths.combined, t("lens_eff_tip")],
  ];
  box.replaceChildren(
    ...cells.map(([label, info, tip]) => {
      const c = el("div", "lens-cell");
      c.title = tip;
      const top = el("div", "lens-top");
      top.append(el("span", "k", label), el("span", "v", `${fmtNum(info.length)} ${t("chars_unit")}`));
      const meter = el("div", "meter");
      const fill = el("i");
      fill.style.width = `${Math.min(100, (info.length / METER_MAX) * 100)}%`;
      fill.style.setProperty("--c", RISK_COLOR[info.risk.level]);
      meter.append(fill);
      for (const tk of METER_TICKS) {
        const tick = el("b");
        tick.style.left = `${(tk / METER_MAX) * 100}%`;
        meter.append(tick);
      }
      c.append(top, meter, el("div", `lens-msg ${RISK_CLASS[info.risk.level]}`, info.risk.message));
      return c;
    }),
  );
}

async function refreshLengths() {
  try {
    lengths = await invoke("get_path_lengths");
  } catch {
    lengths = null; // la jauge disparaît plutôt que d'afficher un chiffre inventé
  }
  renderLens();
}

async function refreshConflicts() {
  try {
    const list = await invoke("analyze_conflicts");
    conflicts = new Map(list.map((c) => [`${c.scope}:${c.index}`, c]));
  } catch {
    conflicts = new Map();
  }
  if (report) renderTable();
}

function renderAdmin() {
  const p = $("#admin-pill");
  p.replaceChildren(icon(isAdmin ? "lock-open" : "lock", 12));
  p.className = `pill ${isAdmin ? "ok" : "warn"}`;
  p.append(isAdmin ? t("admin_ok") : t("admin_need"));
  p.title = isAdmin ? "" : t("admin_need_title");
}

function renderButtons() {
  const n = (p) => (p ? p.targets.length : 0);
  $("#cnt-orphans").textContent = String(n(plans.orphans));
  $("#cnt-dups").textContent = String(n(plans.duplicates));
  $("#btn-orphans").disabled = n(plans.orphans) === 0;
  $("#btn-dups").disabled = n(plans.duplicates) === 0;
  const saved = plans.compress ? plans.compress.saved_chars : 0;
  $("#cnt-compress").textContent = saved ? `−${saved}` : "0";
  $("#btn-compress").disabled = !plans.compress || plans.compress.changes.length === 0;
}

// ─────────────── Toasts & LED WM_SETTINGCHANGE ───────────────

function toast(msg, kind = "ok") {
  const tt = el("div", `toast ${kind}`);
  tt.append(icon(kind === "error" ? "circle-alert" : kind === "warn" ? "triangle-alert" : "check", 14), el("span", "", msg));
  const x = el("button", "x");
  x.setAttribute("aria-label", t("close"));
  x.append(icon("x", 12));
  const close = () => tt.remove();
  x.addEventListener("click", close);
  tt.append(x);
  const box = $("#toasts");
  box.append(tt);
  while (box.children.length > 3) box.firstChild.remove();
  if (kind !== "error") setTimeout(close, kind === "warn" ? 12000 : 7000);
}

function renderSync() {
  const led = $("#led");
  if (sync.kind === "ok") {
    led.className = "led ok";
    $("#sync-txt").textContent = t("sync_ok", { time: fmtTime(sync.time) });
  } else if (sync.kind === "warn") {
    led.className = "led warn";
    $("#sync-txt").textContent = t("sync_expired", { time: fmtTime(sync.time) });
  } else {
    led.className = "led idle";
    $("#sync-txt").textContent = t("sync_idle");
  }
}

/** LED du pied de page + phrase à ajouter au message de l'action. */
function notifySync(broadcast) {
  sync = { kind: broadcast ? "ok" : "warn", time: new Date() };
  renderSync();
  return t(broadcast ? "sync_msg_ok" : "sync_msg_warn");
}

// ─────────────── Modale « diff Git » ───────────────

/** Réduit les longues suites de lignes inchangées à « … N lignes inchangées ». */
function entryDiff(entries, isDeleted, context = 1) {
  const visible = entries.map((_, i) => {
    for (let d = -context; d <= context; d++) if (isDeleted(i + d)) return true;
    return false;
  });
  const out = [];
  let hidden = 0;
  entries.forEach((e, i) => {
    if (visible[i]) {
      if (hidden) out.push({ t: "gap", text: t("diff_gap", { n: hidden }) });
      hidden = 0;
      out.push({ t: isDeleted(i) ? "del" : "ctx", text: e.raw_path });
    } else hidden++;
  });
  if (hidden) out.push({ t: "gap", text: t("diff_gap", { n: hidden }) });
  return out;
}

function diffLine(l) {
  const d = el("div", `dl ${l.t}`);
  if (l.t === "hunk" || l.t === "gap") {
    d.textContent = l.text;
    return d;
  }
  const sym = { del: "−", add: "+", ctx: " ", note: "#" }[l.t];
  const tx = el("span", "tx", l.text);
  if (l.sub) tx.append(el("span", "sub", l.sub));
  d.append(el("span", "sym", sym), tx);
  return d;
}

/** Résout true si l'utilisateur confirme, false sinon. Rien n'est écrit ici. */
function confirmModal({ title, subtitle, diff, warning, note, confirmLabel, canConfirm = true, iconName = "triangle-alert", tone = "danger" }) {
  const dlg = $("#modal");
  $("#m-ok").className = `btn ${tone}`; // danger = suppression ; primary = restauration / application
  $("#m-icon").replaceChildren(icon(iconName, 16));
  $("#m-title").textContent = title;
  $("#m-sub").textContent = subtitle || "";
  const add = diff.filter((l) => l.t === "add").length;
  const del = diff.filter((l) => l.t === "del").length;
  $("#m-stats").replaceChildren(el("span", "stat-del", `−${del}`), " ", el("span", "stat-add", `+${add}`));
  $("#m-alert").classList.toggle("hidden", !warning);
  $("#m-alert-txt").textContent = warning || "";
  $("#m-diff").replaceChildren(...diff.map(diffLine));
  $("#m-note").textContent = note || "";
  const ok = $("#m-ok");
  ok.textContent = confirmLabel;
  ok.disabled = !canConfirm;
  return new Promise((resolve) => {
    const done = (v) => {
      ok.removeEventListener("click", yes);
      $("#m-cancel").removeEventListener("click", no);
      dlg.removeEventListener("cancel", no);
      if (dlg.open) dlg.close();
      resolve(v);
    };
    const yes = () => done(true);
    const no = () => done(false);
    ok.addEventListener("click", yes);
    $("#m-cancel").addEventListener("click", no);
    dlg.addEventListener("cancel", no);
    dlg.showModal();
  });
}

// ─────────────── Actions PATH ───────────────

async function refreshPlans() {
  try {
    const [orphans, duplicates, compress] = await Promise.all([
      invoke("preview_cleanup", { kind: "orphans", scope: scopeSel() }),
      invoke("preview_cleanup", { kind: "duplicates", scope: scopeSel() }),
      invoke("preview_compress", { scope: scopeSel() }),
    ]);
    plans = { orphans, duplicates, compress };
  } catch (err) {
    plans = { orphans: null, duplicates: null, compress: null };
    toast(t("preview_failed", { err }), "error");
  }
  renderButtons();
}

async function onBackup() {
  try {
    const file = await invoke("create_registry_backup");
    toast(t("backup_created", { file }));
    await refreshBackups();
  } catch (err) {
    toast(t("backup_failed", { err }), "error");
  }
}

function cleanupDiff(plan) {
  const lines = [];
  for (const scope of ["System", "User"]) {
    const targets = plan.targets.filter((x) => x.scope === scope);
    if (!targets.length) continue;
    const idx = new Set(targets.map((x) => x.index));
    const entries = report[scope.toLowerCase()].entries;
    lines.push({ t: "hunk", text: `@@ ${HIVE[scope]} · Path @@` });
    lines.push(...entryDiff(entries, (i) => idx.has(i)));
    // Raison de chaque suppression, en commentaire sous le diff du scope.
    for (const x of targets) lines.push({ t: "note", text: `${x.raw_path} : ${x.reason}` });
  }
  return lines;
}

async function onCleanup(kind) {
  const plan = plans[kind];
  if (!plan || plan.targets.length === 0) return;
  const scope = scopeSel();
  const isOrphans = kind === "orphans";
  const blocked = plan.requires_admin && !plan.is_admin;
  const skipped = plan.skipped_unsafe ? t("skipped_unsafe", { n: plan.skipped_unsafe }) : "";
  const ok = await confirmModal({
    title: t(isOrphans ? "del_title_orphans" : "del_title_dups"),
    subtitle: t("del_sub", { n: plan.targets.length, scope: scopeName(scope) }),
    diff: cleanupDiff(plan),
    warning: blocked ? t("admin_block") : "",
    note: t("auto_backup_note", { skipped }),
    confirmLabel: blocked ? t("admin_required_btn") : t("delete_btn"),
    canConfirm: !blocked,
  });
  if (!ok) return;
  try {
    const res = await invoke(isOrphans ? "remove_orphans" : "deduplicate_paths", { scope });
    const done = t("removed_done", { n: res.removed.length, backup: res.backup_file ?? t("no_backup_nothing") });
    if (res.removed.length === 0) toast(done);
    else toast(done + notifySync(res.broadcast), res.broadcast ? "ok" : "warn");
  } catch (err) {
    toast(isAdminError(err) ? `${err}` : t("failed", { err }), "error");
  }
  await refreshAll();
}

function compressDiff(plan) {
  const lines = [];
  for (const scope of ["System", "User"]) {
    const ch = plan.changes.filter((c) => c.scope === scope);
    if (!ch.length) continue;
    lines.push({ t: "hunk", text: `@@ ${HIVE[scope]} · Path @@` });
    for (const c of ch) {
      lines.push({ t: "del", text: c.old });
      lines.push({ t: "add", text: c.new, sub: `−${c.old.length - c.new.length} ${t("chars_unit")}` });
    }
  }
  return lines;
}

async function onCompress() {
  const plan = plans.compress;
  if (!plan || plan.changes.length === 0) return;
  const scope = scopeSel();
  const blocked = plan.requires_admin && !plan.is_admin;
  const ok = await confirmModal({
    title: t("compress_title"),
    subtitle: t("compress_sub", { n: plan.changes.length, saved: plan.saved_chars, scope: scopeName(scope) }),
    diff: compressDiff(plan),
    warning: blocked ? t("admin_block") : "",
    note: t("compress_note"),
    confirmLabel: blocked ? t("admin_required_btn") : t("compress_btn"),
    canConfirm: !blocked,
    iconName: "shrink",
    tone: "primary",
  });
  if (!ok) return;
  try {
    const res = await invoke("compress_path", { scope });
    toast(
      t("compress_done", { saved: res.saved_chars, n: res.changes.length, backup: res.backup_file ?? t("no_backup") }) + notifySync(res.broadcast),
      res.broadcast ? "ok" : "warn",
    );
  } catch (err) {
    toast(isAdminError(err) ? `${err}` : t("failed", { err }), "error");
  }
  await refreshAll();
}

/** Monte/descend une entrée dans son scope ; le texte vu est renvoyé pour détecter un PATH modifié entre-temps. */
async function onMove(e, direction) {
  try {
    const res = await invoke("move_path_entry", { scope: e.scope, index: e.idx, direction, expectedPath: e.raw_path });
    lastMoved = { scope: e.scope, path: e.raw_path };
    notifySync(res.broadcast);
    if (!res.broadcast) toast(t("move_warn"), "warn");
  } catch (err) {
    toast(isAdminError(err) ? `${err}` : t("move_failed", { err }), "error");
  }
  await refreshAll();
}

// ─────────────── Sauvegardes ───────────────

async function refreshBackups() {
  const ul = $("#backups");
  try {
    const list = await invoke("list_backups");
    if (list.length === 0) {
      ul.replaceChildren(el("li", "item", t("no_backups")));
      return;
    }
    ul.replaceChildren(
      ...list.map((b) => {
        const li = el("li", "item");
        const when = new Date(b.created_at);
        const txt = el("div", "");
        txt.append(
          el("div", "t", isNaN(when) ? b.file_name : fmtDate(when)),
          el("div", "d", `${reasonLabel(b.reason)} · HKCU ${b.user_entries} · HKLM ${b.system_entries}`),
        );
        const btn = el("button", "btn sm info");
        btn.append(icon("rotate-ccw", 12), t("restore_btn"));
        btn.addEventListener("click", () => onRestore(b));
        li.append(txt, btn);
        return li;
      }),
    );
  } catch (err) {
    ul.replaceChildren(el("li", "item", t("backups_failed", { err })));
  }
}

function restoreDiff(p) {
  const lines = [];
  for (const scope of ["System", "User"]) {
    const d = p[scope.toLowerCase()];
    if (!d) continue;
    const cur = new Set(d.current.map((x) => x.toLowerCase()));
    const bak = new Set(d.backup.map((x) => x.toLowerCase()));
    lines.push({ t: "hunk", text: `@@ ${HIVE[scope]} · Path @@` });
    const before = lines.length;
    d.current.filter((x) => !bak.has(x.toLowerCase())).forEach((x) => lines.push({ t: "del", text: x }));
    d.backup.filter((x) => !cur.has(x.toLowerCase())).forEach((x) => lines.push({ t: "add", text: x }));
    const same = d.backup.filter((x) => cur.has(x.toLowerCase())).length;
    if (same) lines.push({ t: "gap", text: t("diff_gap", { n: same }) });
    if (lines.length === before + (same ? 1 : 0)) lines.push({ t: "note", text: t("restore_same_order") });
  }
  if (p.variables.length) {
    lines.push({ t: "hunk", text: "@@ HKCU\\Environment · variables @@" });
    for (const v of p.variables) {
      if (v.current !== null) lines.push({ t: "del", text: `${v.name}=${v.current}` });
      if (v.backup !== null) lines.push({ t: "add", text: `${v.name}=${v.backup}` });
      else lines.push({ t: "note", text: t("restore_var_deleted", { name: v.name }) });
    }
  }
  return lines;
}

async function onRestore(b) {
  let p;
  try {
    p = await invoke("preview_restore", { backupFile: b.file_name });
  } catch (err) {
    toast(t("preview_failed", { err }), "error");
    return;
  }
  const diff = restoreDiff(p);
  if (diff.length === 0) {
    toast(t("restore_nothing"));
    return;
  }
  const needsAdmin = !!p.system && !isAdmin;
  const when = new Date(p.created_at);
  const ok = await confirmModal({
    title: t("restore_title"),
    subtitle: t("restore_sub", { when: isNaN(when) ? b.file_name : fmtDate(when), reason: reasonLabel(p.reason) }),
    diff,
    warning: needsAdmin ? t("restore_admin_warn") : "",
    note: t("restore_note"),
    confirmLabel: needsAdmin ? t("admin_required_btn") : t("restore_btn"),
    canConfirm: !needsAdmin,
    iconName: "rotate-ccw",
    tone: "primary",
  });
  if (!ok) return;
  try {
    const synced = await invoke("restore_registry_backup", { backupFile: b.file_name });
    toast(t("restore_done") + notifySync(synced), synced ? "ok" : "warn");
  } catch (err) {
    toast(t("restore_failed", { err }), "error");
  }
  await refreshAll();
}

// ─────────────── Profils & Dotfiles ───────────────

async function refreshPresets() {
  const ul = $("#presets");
  try {
    const presets = await invoke("list_presets");
    ul.replaceChildren(
      ...presets.map((p) => {
        const li = el("li", "item");
        const txt = el("div", "");
        txt.append(el("div", "t", p.name), el("div", "d", p.description));
        const btn = el("button", "btn sm primary", t("preset_apply_btn"));
        btn.addEventListener("click", () => onProfile(p));
        li.append(txt, btn);
        return li;
      }),
    );
  } catch (err) {
    ul.replaceChildren(el("li", "item", t("presets_failed", { err })));
  }
}

// ─────────────── Thème sombre / clair ───────────────

let theme = (() => { try { return localStorage.getItem("theme") === "light" ? "light" : "dark"; } catch { return "dark"; } })();

/** Applique le thème au document ; le bouton montre le thème vers lequel on bascule. */
function applyTheme() {
  document.documentElement.dataset.theme = theme;
  const btn = $("#btn-theme");
  const label = t(theme === "dark" ? "theme_to_light" : "theme_to_dark");
  btn.textContent = theme === "dark" ? "☀️" : "🌙";
  btn.title = label;
  btn.setAttribute("aria-label", label);
}

function toggleTheme() {
  theme = theme === "dark" ? "light" : "dark";
  try { localStorage.setItem("theme", theme); } catch { /* stockage indisponible */ }
  applyTheme();
}

/** Copie dans le presse-papiers un diagnostic du PATH à coller dans un assistant IA. */
async function onDiag() {
  if (!report) return;
  try {
    const version = await window.__TAURI__.app.getVersion();
    await navigator.clipboard.writeText(
      buildPrompt({ version, isAdmin, entries: allEntries(), lengths, conflicts: [...conflicts.values()], numberOf, now: new Date() }),
    );
    toast(t("diag_copied"));
  } catch {
    toast(t("diag_failed"), "error");
  }
}

/** Bouton « Copier » : bascule en « Copié » ; le texte copié vient du backend. */
function copyButton(label, text) {
  const copy = el("button", "btn sm");
  copy.style.marginTop = "8px";
  copy.append(icon("copy", 12), label);
  copy.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(text);
      copy.replaceChildren(icon("check", 12), t("copied"));
    } catch {
      copy.textContent = t("copy_denied");
    }
  });
  return copy;
}

async function onExport() {
  const box = $("#export-result");
  try {
    const r = await invoke("export_profile", { name: $("#profile-name").value, description: $("#profile-desc").value });
    box.replaceChildren(
      el("div", "okc", t("export_done", { path: r.path })),
      el("div", "", t("export_counts", { u: r.path_user_count, s: r.path_system_count, v: r.variable_count })),
    );
    if (r.skipped_secrets.length) box.append(el("div", "wc", t("skipped_secrets", { list: r.skipped_secrets.join(", ") })));
    box.append(copyButton(t("copy_json"), r.json));
    box.classList.remove("hidden");
    toast(t("export_toast"));
  } catch (err) {
    toast(t("export_failed", { err }), "error");
  }
}

async function onImportFile(file) {
  if (!file) return;
  try {
    const profile = await invoke("import_profile", { json: await file.text() });
    await onProfile(profile);
  } catch (err) {
    toast(t("import_failed", { err }), "error");
  } finally {
    $("#file-import").value = ""; // permet de re-choisir le même fichier
  }
}

async function onExportScript() {
  const box = $("#script-result");
  try {
    const r = await invoke("export_powershell", { name: $("#profile-name").value });
    box.replaceChildren(
      el("div", "okc", t("script_done", { path: r.path })),
      el("div", "", t("script_counts", { u: r.path_count, v: r.variable_count })),
      el("div", "", "powershell -NoProfile -ExecutionPolicy Bypass -File .\\setup-env.ps1 -WhatIf"),
    );
    if (r.skipped_secrets.length) box.append(el("div", "wc", t("skipped_secrets", { list: r.skipped_secrets.join(", ") })));
    box.append(copyButton(t("copy_script"), r.script));
    box.classList.remove("hidden");
    toast(t("script_toast"));
  } catch (err) {
    toast(t("script_failed", { err }), "error");
  }
}

function profileDiff(plan) {
  const lines = [];
  if (plan.path_added.length || plan.path_removed.length) {
    lines.push({ t: "hunk", text: `@@ ${HIVE.User} · Path @@` });
    plan.path_removed.forEach((x) => lines.push({ t: "del", text: x }));
    plan.path_added.forEach((x) => lines.push({ t: "add", text: x }));
  }
  if (plan.variables.length) {
    lines.push({ t: "hunk", text: "@@ HKCU\\Environment · variables @@" });
    for (const v of plan.variables) {
      if (v.old !== null) lines.push({ t: "del", text: `${v.name}=${v.old}` });
      lines.push({ t: "add", text: `${v.name}=${v.new}` });
    }
  }
  plan.notes.forEach((n) => lines.push({ t: "note", text: n }));
  return lines;
}

/** Aperçu → modale diff → application (le Rust recalcule le plan et sauvegarde). */
async function onProfile(profile) {
  let plan;
  try {
    plan = await invoke("preview_profile", { profile });
  } catch (err) {
    toast(t("profile_refused", { err }), "error");
    return;
  }
  if (plan.new_user_path === null && plan.variables.length === 0) {
    toast(t("profile_nothing", { name: plan.name }) + (plan.notes.length ? ` (${plan.notes.join(" ; ")})` : ""));
    return;
  }
  const ok = await confirmModal({
    title: t("profile_title", { name: plan.name }),
    subtitle: t("profile_sub", { a: plan.path_added.length, r: plan.path_removed.length, v: plan.variables.length }),
    diff: profileDiff(plan),
    warning: plan.system_entries_ignored ? t("profile_sys_warn", { n: plan.system_entries_ignored }) : "",
    note: t("profile_note"),
    confirmLabel: t("apply_btn"),
    iconName: "braces",
    tone: "primary",
  });
  if (!ok) return;
  try {
    const res = await invoke("apply_profile", { profile });
    toast(
      res.changed
        ? t("profile_applied", { vars: res.variables_changed, path: t(res.path_changed ? "path_changed" : "path_unchanged"), backup: res.backup_file }) +
            notifySync(res.broadcast)
        : t("nothing_to_change"),
      res.broadcast || !res.changed ? "ok" : "warn",
    );
  } catch (err) {
    toast(t("failed", { err }), "error");
  }
  await refreshAll();
}

// ─────────────── Navigation & chargement ───────────────

function showView(name) {
  const path = name === "path";
  $("#view-path").classList.toggle("hidden", !path);
  $("#view-profiles").classList.toggle("hidden", path);
  $("#nav-path").setAttribute("aria-selected", String(path));
  $("#nav-profiles").setAttribute("aria-selected", String(!path));
  if (path && report) renderTable();
  else $("#count-txt").textContent = "";
}

async function refreshAll() {
  try {
    [report, isAdmin] = await Promise.all([invoke("get_registry_paths"), invoke("is_admin")]);
    renderAdmin();
    renderTable();
    for (const k of ["user", "system"]) {
      if (report[k].error) toast(t("read_path_failed", { scope: scopeName(k === "user" ? "User" : "System"), err: report[k].error }), "error");
    }
  } catch (err) {
    toast(t("read_registry_failed", { err }), "error");
  }
  await Promise.all([refreshPlans(), refreshBackups(), refreshLengths(), refreshConflicts()]);
}

// ── Langue : sélecteur FR / EN, transmis au backend (erreurs, risques, presets, script exporté) ──
async function applyLanguage() {
  applyStatic();
  applyTheme();
  $("#lang").value = getLang();
  const name = $("#profile-name");
  if (DEFAULT_PROFILE_NAMES.includes(name.value)) name.value = t("profile_name_default"); // nom par défaut non modifié
  renderSync();
  renderAdmin();
  if (report) renderTable();
  try {
    await invoke("set_language", { lang: getLang() });
  } catch { /* le backend reste en français */ }
  // Textes venant du backend (risques, presets, raisons de nettoyage) : on les redemande.
  await Promise.all([refreshPresets(), refreshAll()]);
}

window.addEventListener("DOMContentLoaded", () => {
  hydrateIcons();
  window.__TAURI__.app?.getVersion?.().then((v) => ($("#ver").textContent = `v${v}`)).catch(() => {});

  $("#nav-path").addEventListener("click", () => showView("path"));
  $("#nav-profiles").addEventListener("click", () => showView("profiles"));
  $("#lang").addEventListener("change", (e) => {
    setLang(e.target.value);
    applyLanguage();
  });

  $("#q").addEventListener("input", (e) => setFilter("q", e.target.value));
  $("#q-clear").addEventListener("click", () => {
    $("#q").value = "";
    setFilter("q", "");
    $("#q").focus();
  });
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      showView("path");
      $("#q").focus();
      $("#q").select();
    }
  });
  for (const [id, key] of [["#chips-state", "state"], ["#chips-scope", "scope"]]) {
    $(id).addEventListener("click", (e) => {
      const c = e.target.closest(".chip");
      if (c) setFilter(key, c.dataset.v);
    });
  }

  $("#btn-backup").addEventListener("click", onBackup);
  $("#btn-orphans").addEventListener("click", () => onCleanup("orphans"));
  $("#btn-dups").addEventListener("click", () => onCleanup("duplicates"));
  $("#btn-compress").addEventListener("click", onCompress);
  $("#btn-diag").addEventListener("click", onDiag);
  $("#btn-theme").addEventListener("click", toggleTheme);
  $("#btn-script").addEventListener("click", onExportScript);
  $("#scope-select").addEventListener("change", refreshPlans);

  $("#btn-export").addEventListener("click", onExport);
  $("#btn-import").addEventListener("click", () => $("#file-import").click());
  $("#file-import").addEventListener("change", (e) => onImportFile(e.target.files[0]));

  applyLanguage(); // textes statiques, set_language, puis presets et données
});
