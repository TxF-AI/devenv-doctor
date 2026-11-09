import "./styles.css";
import { icon, hydrateIcons } from "./icons.js";

const { invoke } = window.__TAURI__.core;

const SCOPE_NAMES = { User: "Utilisateur", System: "Système", All: "Tout" };
const HIVE = { User: "HKCU\\Environment", System: "HKLM\\…\\Session Manager\\Environment" };

let report = null;
let isAdmin = false;
let plans = { orphans: null, duplicates: null };
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
const fmtTime = (d = new Date()) => d.toLocaleTimeString("fr-FR");

// ─────────────── Données & filtres ───────────────

/** Ordre de résolution Windows : Système puis Utilisateur. */
function allEntries() {
  return [...report.system.entries, ...report.user.entries].map((e, i) => ({ ...e, n: i + 1 }));
}

function matches(e) {
  if (filters.scope !== "all" && e.scope !== filters.scope) return false;
  if (filters.state === "ok" && !e.exists) return false;
  if (filters.state === "orphan" && e.exists) return false;
  if (filters.state === "dup" && !e.is_duplicate) return false;
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
    ["Total", all.length, "all", "var(--info)"],
    ["Valides", all.filter((e) => e.exists).length, "ok", "var(--ok)"],
    ["Orphelins", all.filter((e) => !e.exists).length, "orphan", "var(--bad)"],
    ["Doublons", all.filter((e) => e.is_duplicate).length, "dup", "var(--warn)"],
  ];
  $("#stats").replaceChildren(
    ...c.map(([k, v, state, color]) => {
      const b = el("button", "stat");
      b.style.setProperty("--c", color);
      b.setAttribute("aria-pressed", String(filters.state === state));
      b.title = `Filtrer : ${k.toLowerCase()}`;
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
  st.append(e.exists ? badge("Valide", "ok") : badge("Orphelin", "bad", "circle-alert"));
  if (e.is_duplicate) st.append(badge("Doublon", "warn", "copy"));
  const sc = el("td", "sc");
  sc.append(badge(e.scope === "System" ? "HKLM" : "HKCU", "scope"));
  const p = el("td", "path");
  p.append(el("div", "raw", e.raw_path));
  if (e.expanded_path !== e.raw_path) p.append(el("div", "exp", e.expanded_path));
  tr.append(st, sc, p);
  return tr;
}

function renderTable() {
  const all = allEntries();
  const shown = all.filter(matches);
  $("#rows").replaceChildren(...shown.map(row));
  $("#empty").classList.toggle("hidden", shown.length > 0);
  $("#count-txt").textContent = `${shown.length}/${all.length} entrées affichées`;
  renderStats(all);
  $("#q-clear").classList.toggle("hidden", filters.q === "");
  for (const [id, key] of [["#chips-state", "state"], ["#chips-scope", "scope"]]) {
    $(id).querySelectorAll(".chip").forEach((c) => c.setAttribute("aria-pressed", String(c.dataset.v === filters[key])));
  }
}

function setFilter(key, value) {
  filters[key] = value;
  if (report) renderTable();
}

function renderAdmin() {
  const p = $("#admin-pill");
  p.replaceChildren(icon(isAdmin ? "lock-open" : "lock", 12));
  p.className = `pill ${isAdmin ? "ok" : "warn"}`;
  p.append(isAdmin ? "Administrateur : PATH Système modifiable" : "Droits Administrateur requis pour modifier le PATH Système");
  p.title = isAdmin ? "" : "Le PATH Utilisateur reste modifiable (portée « Utilisateur »).";
}

function renderButtons() {
  const n = (p) => (p ? p.targets.length : 0);
  $("#cnt-orphans").textContent = String(n(plans.orphans));
  $("#cnt-dups").textContent = String(n(plans.duplicates));
  $("#btn-orphans").disabled = n(plans.orphans) === 0;
  $("#btn-dups").disabled = n(plans.duplicates) === 0;
}

// ─────────────── Toasts & LED WM_SETTINGCHANGE ───────────────

function toast(msg, kind = "ok") {
  const t = el("div", `toast ${kind}`);
  t.append(icon(kind === "error" ? "circle-alert" : kind === "warn" ? "triangle-alert" : "check", 14), el("span", "", msg));
  const x = el("button", "x");
  x.setAttribute("aria-label", "Fermer");
  x.append(icon("x", 12));
  const close = () => t.remove();
  x.addEventListener("click", close);
  t.append(x);
  const box = $("#toasts");
  box.append(t);
  while (box.children.length > 3) box.firstChild.remove();
  if (kind !== "error") setTimeout(close, kind === "warn" ? 12000 : 7000);
}

/** LED du pied de page + phrase à ajouter au message de l'action. */
function notifySync(broadcast) {
  const led = $("#led");
  if (broadcast) {
    led.className = "led ok";
    $("#sync-txt").textContent = `WM_SETTINGCHANGE · diffusé à ${fmtTime()} · variables synchronisées`;
    return " Variables Windows synchronisées en temps réel.";
  }
  led.className = "led warn";
  $("#sync-txt").textContent = `WM_SETTINGCHANGE · diffusion expirée à ${fmtTime()}`;
  return " Registre modifié, mais la diffusion à Windows n'a pas abouti : relance tes terminaux pour voir le changement.";
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
      if (hidden) out.push({ t: "gap", text: `… ${hidden} entrée(s) inchangée(s)` });
      hidden = 0;
      out.push({ t: isDeleted(i) ? "del" : "ctx", text: e.raw_path });
    } else hidden++;
  });
  if (hidden) out.push({ t: "gap", text: `… ${hidden} entrée(s) inchangée(s)` });
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
    const [orphans, duplicates] = await Promise.all([
      invoke("preview_cleanup", { kind: "orphans", scope: scopeSel() }),
      invoke("preview_cleanup", { kind: "duplicates", scope: scopeSel() }),
    ]);
    plans = { orphans, duplicates };
  } catch (err) {
    plans = { orphans: null, duplicates: null };
    toast(`Aperçu impossible : ${err}`, "error");
  }
  renderButtons();
}

async function onBackup() {
  try {
    const file = await invoke("create_registry_backup");
    toast(`Sauvegarde créée : ${file}`);
    await refreshBackups();
  } catch (err) {
    toast(`Sauvegarde impossible : ${err}`, "error");
  }
}

function cleanupDiff(plan) {
  const lines = [];
  for (const scope of ["System", "User"]) {
    const targets = plan.targets.filter((t) => t.scope === scope);
    if (!targets.length) continue;
    const idx = new Set(targets.map((t) => t.index));
    const entries = report[scope.toLowerCase()].entries;
    lines.push({ t: "hunk", text: `@@ ${HIVE[scope]} · Path @@` });
    lines.push(...entryDiff(entries, (i) => idx.has(i)));
    // Raison de chaque suppression, en commentaire sous le diff du scope.
    for (const t of targets) lines.push({ t: "note", text: `${t.raw_path} : ${t.reason}` });
  }
  return lines;
}

async function onCleanup(kind) {
  const plan = plans[kind];
  if (!plan || plan.targets.length === 0) return;
  const scope = scopeSel();
  const isOrphans = kind === "orphans";
  const blocked = plan.requires_admin && !plan.is_admin;
  const skipped = plan.skipped_unsafe
    ? ` ${plan.skipped_unsafe} entrée(s) introuvable(s) épargnée(s) par prudence (variable non résolue ou chemin réseau).`
    : "";
  const ok = await confirmModal({
    title: isOrphans ? "Supprimer les entrées orphelines" : "Supprimer les doublons",
    subtitle: `${plan.targets.length} entrée(s) · portée : ${SCOPE_NAMES[scope]}`,
    diff: cleanupDiff(plan),
    warning: blocked
      ? "Droits Administrateur requis : des entrées du PATH Système sont concernées. Relance l'application en administrateur, ou choisis la portée « Utilisateur »."
      : "",
    note: `Sauvegarde automatique avant écriture.${skipped}`,
    confirmLabel: blocked ? "Admin requis" : "Supprimer",
    canConfirm: !blocked,
  });
  if (!ok) return;
  try {
    const res = await invoke(isOrphans ? "remove_orphans" : "deduplicate_paths", { scope });
    const done = `${res.removed.length} entrée(s) supprimée(s). Sauvegarde : ${res.backup_file ?? "aucune (rien à faire)"}.`;
    if (res.removed.length === 0) toast(done);
    else toast(done + notifySync(res.broadcast), res.broadcast ? "ok" : "warn");
  } catch (err) {
    toast(isAdminError(err) ? `${err}` : `Échec : ${err}`, "error");
  }
  await refreshAll();
}

// ─────────────── Sauvegardes ───────────────

async function refreshBackups() {
  const ul = $("#backups");
  try {
    const list = await invoke("list_backups");
    if (list.length === 0) {
      ul.replaceChildren(el("li", "item", "Aucune sauvegarde pour l'instant."));
      return;
    }
    ul.replaceChildren(
      ...list.map((b) => {
        const li = el("li", "item");
        const when = new Date(b.created_at);
        const txt = el("div", "");
        txt.append(
          el("div", "t", isNaN(when) ? b.file_name : when.toLocaleString("fr-FR")),
          el("div", "d", `${b.reason} · HKCU ${b.user_entries} · HKLM ${b.system_entries}`),
        );
        const btn = el("button", "btn sm info");
        btn.append(icon("rotate-ccw", 12), "Restaurer");
        btn.addEventListener("click", () => onRestore(b));
        li.append(txt, btn);
        return li;
      }),
    );
  } catch (err) {
    ul.replaceChildren(el("li", "item", `Liste des sauvegardes impossible : ${err}`));
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
    if (same) lines.push({ t: "gap", text: `… ${same} entrée(s) inchangée(s)` });
    if (lines.length === before + (same ? 1 : 0)) lines.push({ t: "note", text: "Mêmes entrées : seuls l'ordre ou le formatage changent." });
  }
  if (p.variables.length) {
    lines.push({ t: "hunk", text: "@@ HKCU\\Environment · variables @@" });
    for (const v of p.variables) {
      if (v.current !== null) lines.push({ t: "del", text: `${v.name}=${v.current}` });
      if (v.backup !== null) lines.push({ t: "add", text: `${v.name}=${v.backup}` });
      else lines.push({ t: "note", text: `${v.name} sera supprimée (elle n'existait pas à l'époque)` });
    }
  }
  return lines;
}

async function onRestore(b) {
  let p;
  try {
    p = await invoke("preview_restore", { backupFile: b.file_name });
  } catch (err) {
    toast(`Aperçu impossible : ${err}`, "error");
    return;
  }
  const diff = restoreDiff(p);
  if (diff.length === 0) {
    toast("Cette sauvegarde correspond déjà à l'état actuel : rien à restaurer.");
    return;
  }
  const needsAdmin = !!p.system && !isAdmin;
  const when = new Date(p.created_at);
  const ok = await confirmModal({
    title: "Restaurer cette sauvegarde",
    subtitle: `État du ${isNaN(when) ? b.file_name : when.toLocaleString("fr-FR")} · ${p.reason}`,
    diff,
    warning: needsAdmin ? "Droits Administrateur requis : le PATH Système a changé depuis cette sauvegarde." : "",
    note: "L'état actuel est sauvegardé avant la restauration (annulable).",
    confirmLabel: needsAdmin ? "Admin requis" : "Restaurer",
    canConfirm: !needsAdmin,
    iconName: "rotate-ccw",
    tone: "primary",
  });
  if (!ok) return;
  try {
    const synced = await invoke("restore_registry_backup", { backupFile: b.file_name });
    toast("Sauvegarde restaurée." + notifySync(synced), synced ? "ok" : "warn");
  } catch (err) {
    toast(`Restauration impossible : ${err}`, "error");
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
        const btn = el("button", "btn sm primary", "Aperçu & appliquer");
        btn.addEventListener("click", () => onProfile(p));
        li.append(txt, btn);
        return li;
      }),
    );
  } catch (err) {
    ul.replaceChildren(el("li", "item", `Presets indisponibles : ${err}`));
  }
}

async function onExport() {
  const box = $("#export-result");
  try {
    const r = await invoke("export_profile", { name: $("#profile-name").value, description: $("#profile-desc").value });
    box.replaceChildren(
      el("div", "okc", `Profil exporté : ${r.path}`),
      el("div", "", `${r.path_user_count} entrée(s) PATH Utilisateur · ${r.path_system_count} PATH Système (informatif) · ${r.variable_count} variable(s)`),
    );
    if (r.skipped_secrets.length) box.append(el("div", "wc", `Exclues (nom évoquant un secret) : ${r.skipped_secrets.join(", ")}`));
    const copy = el("button", "btn sm");
    copy.style.marginTop = "8px";
    copy.append(icon("copy", 12), "Copier le JSON");
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(r.json);
        copy.replaceChildren(icon("check", 12), "Copié");
      } catch {
        copy.textContent = "Copie refusée : ouvre le fichier ci-dessus";
      }
    });
    box.append(copy);
    box.classList.remove("hidden");
    toast("Profil exporté, sans secret ni nom d'utilisateur : prêt pour un dépôt dotfiles.");
  } catch (err) {
    toast(`Export impossible : ${err}`, "error");
  }
}

async function onImportFile(file) {
  if (!file) return;
  try {
    const profile = await invoke("import_profile", { json: await file.text() });
    await onProfile(profile);
  } catch (err) {
    toast(`Import impossible : ${err}`, "error");
  } finally {
    $("#file-import").value = ""; // permet de re-choisir le même fichier
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
    toast(`Profil refusé : ${err}`, "error");
    return;
  }
  if (plan.new_user_path === null && plan.variables.length === 0) {
    toast(`Profil « ${plan.name} » : rien à modifier, ton environnement est déjà conforme.` + (plan.notes.length ? ` (${plan.notes.join(" ; ")})` : ""));
    return;
  }
  const ok = await confirmModal({
    title: `Appliquer le profil « ${plan.name} »`,
    subtitle: `${plan.path_added.length} ajout(s), ${plan.path_removed.length} retrait(s) dans le PATH Utilisateur · ${plan.variables.length} variable(s)`,
    diff: profileDiff(plan),
    warning: plan.system_entries_ignored ? `${plan.system_entries_ignored} entrée(s) PATH Système du profil sont ignorées (jamais appliquées).` : "",
    note: "Seul HKCU est modifié. Sauvegarde automatique avant l'écriture (variables comprises).",
    confirmLabel: "Appliquer",
    iconName: "braces",
    tone: "primary",
  });
  if (!ok) return;
  try {
    const res = await invoke("apply_profile", { profile });
    toast(
      res.changed
        ? `Profil appliqué (${res.variables_changed} variable(s), PATH ${res.path_changed ? "modifié" : "inchangé"}). Sauvegarde : ${res.backup_file}.` + notifySync(res.broadcast)
        : "Rien à modifier.",
      res.broadcast || !res.changed ? "ok" : "warn",
    );
  } catch (err) {
    toast(`Échec : ${err}`, "error");
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
      if (report[k].error) toast(`Lecture du PATH ${k === "user" ? "Utilisateur" : "Système"} : ${report[k].error}`, "error");
    }
  } catch (err) {
    toast(`Lecture du Registre impossible : ${err}`, "error");
  }
  await Promise.all([refreshPlans(), refreshBackups()]);
}

window.addEventListener("DOMContentLoaded", () => {
  hydrateIcons();
  window.__TAURI__.app?.getVersion?.().then((v) => ($("#ver").textContent = `v${v}`)).catch(() => {});

  $("#nav-path").addEventListener("click", () => showView("path"));
  $("#nav-profiles").addEventListener("click", () => showView("profiles"));

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
  $("#scope-select").addEventListener("change", refreshPlans);

  $("#btn-export").addEventListener("click", onExport);
  $("#btn-import").addEventListener("click", () => $("#file-import").click());
  $("#file-import").addEventListener("change", (e) => onImportFile(e.target.files[0]));

  refreshPresets();
  refreshAll();
});
