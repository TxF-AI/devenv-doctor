import "./styles.css";

const { invoke } = window.__TAURI__.core;

const SCOPES = {
  user: { label: "PATH Utilisateur (HKCU)", tag: "HKCU" },
  system: { label: "PATH Système (HKLM)", tag: "HKLM" },
};
const SCOPE_NAMES = { User: "Utilisateur", System: "Système", All: "Tout" };

let report = null;
let active = "user";
let isAdmin = false;
let plans = { orphans: null, duplicates: null };

const $ = (sel) => document.querySelector(sel);

// Toujours textContent : les chemins viennent du Registre, jamais d'HTML injecté.
function el(tagName, cls, text) {
  const n = document.createElement(tagName);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}

function badge(text, cls) {
  return el("span", `ml-2 rounded px-2 py-0.5 text-xs font-bold uppercase ${cls}`, text);
}

const isAdminError = (err) => String(err).startsWith("AdminRequired");
const scopeSel = () => $("#scope-select").value;

// ─────────────── Liste des entrées ───────────────

function row(e) {
  const ok = e.exists;
  const li = el(
    "li",
    "rounded border px-3 py-2 break-all " +
      (ok
        ? "border-emerald-500/40 bg-emerald-500/5 text-emerald-300"
        : "border-rose-500 bg-rose-500/10 text-rose-300 shadow-[0_0_10px_rgba(244,63,94,.5)]"),
  );
  const top = el("div", "flex items-start justify-between gap-2");
  top.append(el("span", "", e.raw_path));
  const tags = el("span", "shrink-0");
  tags.append(ok ? badge("Valide", "bg-emerald-500 text-zinc-950") : badge("Orphelin", "bg-rose-500 text-zinc-950"));
  if (e.is_duplicate) tags.append(badge("Doublon", "bg-amber-400 text-zinc-950"));
  tags.append(badge(SCOPES[active].tag, "bg-zinc-700 text-zinc-200"));
  top.append(tags);
  li.append(top);
  if (e.expanded_path !== e.raw_path) {
    li.append(el("div", "mt-1 text-xs text-zinc-400", `→ ${e.expanded_path}`));
  }
  return li;
}

function renderCounters(all) {
  const c = {
    Total: all.length,
    Valides: all.filter((e) => e.exists).length,
    Orphelins: all.filter((e) => !e.exists).length,
    Doublons: all.filter((e) => e.is_duplicate).length,
  };
  const colors = { Total: "text-fuchsia-300", Valides: "text-emerald-300", Orphelins: "text-rose-300", Doublons: "text-amber-300" };
  $("#counters").replaceChildren(
    ...Object.entries(c).map(([k, v]) => {
      const d = el("div", "rounded border border-zinc-800 bg-zinc-900 p-2");
      d.append(el("div", `text-2xl font-bold ${colors[k]}`, String(v)), el("div", "text-xs uppercase text-zinc-500", k));
      return d;
    }),
  );
}

function renderList() {
  renderCounters([...report.user.entries, ...report.system.entries]);
  for (const key of Object.keys(SCOPES)) {
    const b = $(`#tab-${key}`);
    b.textContent = `${SCOPES[key].label} · ${report[key].entries.length}`;
    b.setAttribute("aria-selected", String(key === active));
    b.className =
      "rounded border px-3 py-1 text-sm " +
      (key === active
        ? "border-fuchsia-400 bg-fuchsia-400/10 text-fuchsia-300"
        : "border-zinc-700 text-zinc-400 hover:border-zinc-500");
  }
  const r = report[active];
  const meta = $("#meta");
  meta.textContent = r.error
    ? `Erreur de lecture : ${r.error}`
    : r.value_type
      ? `Type de valeur : ${r.value_type}` + (r.value_type === "REG_SZ" ? " (les %VAR% ne sont pas développées par Windows)" : "")
      : "Valeur Path absente du Registre.";
  meta.className = "mb-3 text-xs " + (r.error ? "text-rose-400" : "text-zinc-500");
  $("#entries").replaceChildren(...r.entries.map(row));
}

// ─────────────── Bannière admin, boutons, sauvegardes ───────────────

function renderBanner() {
  const b = $("#admin-banner");
  if (isAdmin) {
    b.className = "mb-3 rounded border border-emerald-500/50 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300";
    b.textContent = "🔓 Mode Administrateur : le PATH Système est modifiable.";
  } else {
    b.className = "mb-3 rounded border border-amber-400 bg-amber-400/10 px-3 py-2 text-sm text-amber-300";
    b.textContent =
      "🔒 Droits Administrateur requis pour modifier le PATH Système. Le PATH Utilisateur reste modifiable (choisir la portée « Utilisateur »).";
  }
}

function renderButtons() {
  const n = (p) => (p ? p.targets.length : 0);
  const o = $("#btn-orphans");
  const d = $("#btn-dups");
  o.textContent = `🧹 Supprimer les Orphelins (${n(plans.orphans)})`;
  d.textContent = `🔄 Dédoublonner (${n(plans.duplicates)})`;
  o.disabled = n(plans.orphans) === 0;
  d.disabled = n(plans.duplicates) === 0;
}

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

async function refreshBackups() {
  const ul = $("#backups");
  try {
    const list = await invoke("list_backups");
    if (list.length === 0) {
      ul.replaceChildren(el("li", "text-zinc-500", "Aucune sauvegarde pour l'instant."));
      return;
    }
    ul.replaceChildren(
      ...list.map((b) => {
        const li = el("li", "flex items-center justify-between gap-2 rounded border border-zinc-800 bg-zinc-900 px-3 py-1.5");
        const when = new Date(b.created_at);
        const label = isNaN(when) ? b.file_name : when.toLocaleString("fr-FR");
        li.append(el("span", "text-zinc-300", `${label} · ${b.reason} · U:${b.user_entries} S:${b.system_entries}`));
        const btn = el("button", "shrink-0 rounded border border-cyan-400 px-2 py-0.5 text-xs text-cyan-300 hover:bg-cyan-400/10", "Restaurer");
        btn.addEventListener("click", () => onRestore(b));
        li.append(btn);
        return li;
      }),
    );
  } catch (err) {
    ul.replaceChildren(el("li", "text-rose-400", `Liste des sauvegardes impossible : ${err}`));
  }
}

let toastTimer;
function toast(msg, kind = "ok") {
  const t = $("#toast");
  t.textContent = msg;
  t.className =
    "mb-3 rounded border px-3 py-2 text-sm break-all " +
    (kind === "error"
      ? "border-rose-500 bg-rose-500/10 text-rose-300"
      : "border-cyan-400 bg-cyan-400/10 text-cyan-300");
  clearTimeout(toastTimer);
  if (kind !== "error") toastTimer = setTimeout(() => t.classList.add("hidden"), 12000);
}

// ─────────────── Modale de confirmation ───────────────

/** Résout true si l'utilisateur confirme, false sinon. Rien n'est écrit ici. */
function confirmModal({ title, intro, items, warning, note, confirmLabel, canConfirm = true }) {
  const dlg = $("#modal");
  $("#modal-title").textContent = title;
  $("#modal-intro").textContent = intro;
  const w = $("#modal-warning");
  w.textContent = warning || "";
  w.classList.toggle("hidden", !warning);
  $("#modal-list").replaceChildren(
    ...items.map((it) => {
      const li = el("li", "rounded border border-zinc-700 bg-zinc-950 px-2 py-1 break-all");
      li.append(el("span", "text-zinc-200", it.text));
      if (it.sub) li.append(el("div", "text-zinc-500", it.sub));
      return li;
    }),
  );
  $("#modal-note").textContent = note || "";
  const ok = $("#modal-confirm");
  ok.textContent = confirmLabel;
  ok.disabled = !canConfirm;
  return new Promise((resolve) => {
    const done = (v) => {
      ok.removeEventListener("click", yes);
      $("#modal-cancel").removeEventListener("click", no);
      dlg.removeEventListener("cancel", no);
      if (dlg.open) dlg.close();
      resolve(v);
    };
    const yes = () => done(true);
    const no = () => done(false);
    ok.addEventListener("click", yes);
    $("#modal-cancel").addEventListener("click", no);
    dlg.addEventListener("cancel", no);
    dlg.showModal();
  });
}

// ─────────────── Actions ───────────────

async function onBackup() {
  try {
    const file = await invoke("create_registry_backup");
    toast(`Sauvegarde créée : ${file}`);
    await refreshBackups();
  } catch (err) {
    toast(`Sauvegarde impossible : ${err}`, "error");
  }
}

async function onCleanup(kind) {
  const plan = plans[kind];
  if (!plan || plan.targets.length === 0) return;
  const scope = scopeSel();
  const isOrphans = kind === "orphans";
  const blocked = plan.requires_admin && !plan.is_admin;
  const skipped = plan.skipped_unsafe
    ? ` ${plan.skipped_unsafe} entrée(s) introuvable(s) sont épargnée(s) par prudence (variable non résolue ou chemin réseau).`
    : "";
  const ok = await confirmModal({
    title: isOrphans ? "Supprimer les entrées orphelines ?" : "Supprimer les doublons ?",
    intro: `${plan.targets.length} entrée(s) seront retirées du Registre (portée : ${SCOPE_NAMES[scope]}).`,
    items: plan.targets.map((t) => ({
      text: `[${t.scope === "System" ? "HKLM" : "HKCU"}] ${t.raw_path}`,
      sub: `${t.reason}${t.expanded_path !== t.raw_path ? ` → ${t.expanded_path}` : ""}`,
    })),
    warning: blocked
      ? "Droits Administrateur requis : des entrées du PATH Système sont concernées. Relance l'application en administrateur, ou choisis la portée « Utilisateur »."
      : "",
    note: `Une sauvegarde automatique est créée juste avant l'écriture, restaurable en un clic.${skipped} Les programmes déjà ouverts ne voient le changement qu'après redémarrage.`,
    confirmLabel: blocked ? "Admin requis" : "Confirmer la suppression",
    canConfirm: !blocked,
  });
  if (!ok) return;
  try {
    const res = await invoke(isOrphans ? "remove_orphans" : "deduplicate_paths", { scope });
    toast(`${res.removed.length} entrée(s) supprimée(s). Sauvegarde : ${res.backup_file ?? "aucune (rien à faire)"}`);
  } catch (err) {
    toast(isAdminError(err) ? `${err}` : `Échec : ${err}`, "error");
  }
  await refreshAll();
}

async function onRestore(b) {
  const ok = await confirmModal({
    title: "Restaurer cette sauvegarde ?",
    intro: `Le PATH sera remis dans l'état du ${new Date(b.created_at).toLocaleString("fr-FR")}.`,
    items: [
      { text: `PATH Utilisateur : ${b.user_entries} entrée(s)` },
      { text: `PATH Système : ${b.system_entries} entrée(s)`, sub: isAdmin ? "" : "Admin requis si le PATH Système a changé depuis" },
    ],
    warning: "",
    note: "L'état actuel est sauvegardé automatiquement avant la restauration, pour pouvoir l'annuler.",
    confirmLabel: "Restaurer",
  });
  if (!ok) return;
  try {
    await invoke("restore_registry_backup", { backupFile: b.file_name });
    toast("Sauvegarde restaurée.");
  } catch (err) {
    toast(`Restauration impossible : ${err}`, "error");
  }
  await refreshAll();
}

// ─────────────── Chargement ───────────────

async function refreshAll() {
  try {
    [report, isAdmin] = await Promise.all([invoke("get_registry_paths"), invoke("is_admin")]);
    renderBanner();
    renderList();
  } catch (err) {
    $("#meta").textContent = `Erreur : ${err}`;
  }
  await Promise.all([refreshPlans(), refreshBackups()]);
}

window.addEventListener("DOMContentLoaded", () => {
  for (const key of Object.keys(SCOPES)) {
    $(`#tab-${key}`).addEventListener("click", () => {
      active = key;
      if (report) renderList();
    });
  }
  $("#btn-backup").addEventListener("click", onBackup);
  $("#btn-orphans").addEventListener("click", () => onCleanup("orphans"));
  $("#btn-dups").addEventListener("click", () => onCleanup("duplicates"));
  $("#scope-select").addEventListener("change", refreshPlans);
  refreshAll();
});
