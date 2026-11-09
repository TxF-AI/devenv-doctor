import "./styles.css";

const { invoke } = window.__TAURI__.core;

const SCOPES = {
  user: { label: "PATH Utilisateur (HKCU)", tag: "HKCU" },
  system: { label: "PATH Système (HKLM)", tag: "HKLM" },
};

let report = null;
let active = "user";

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

function status(e) {
  return { ok: e.exists, orphan: !e.exists, dup: e.is_duplicate };
}

function row(e) {
  const s = status(e);
  const li = el(
    "li",
    "rounded border px-3 py-2 break-all " +
      (s.ok
        ? "border-emerald-500/40 bg-emerald-500/5 text-emerald-300"
        : "border-rose-500 bg-rose-500/10 text-rose-300 shadow-[0_0_10px_rgba(244,63,94,.5)]"),
  );
  const top = el("div", "flex items-start justify-between gap-2");
  top.append(el("span", "", e.raw_path));
  const tags = el("span", "shrink-0");
  tags.append(s.ok ? badge("Valide", "bg-emerald-500 text-zinc-950") : badge("Orphelin", "bg-rose-500 text-zinc-950"));
  if (s.dup) tags.append(badge("Doublon", "bg-amber-400 text-zinc-950"));
  tags.append(badge(SCOPES[active].tag, "bg-zinc-700 text-zinc-200"));
  top.append(tags);
  li.append(top);
  if (e.expanded_path !== e.raw_path) {
    li.append(el("div", "mt-1 text-xs text-zinc-400", `→ ${e.expanded_path}`));
  }
  return li;
}

function counters(all) {
  const c = {
    Total: all.length,
    Valides: all.filter((e) => e.exists).length,
    Orphelins: all.filter((e) => !e.exists).length,
    Doublons: all.filter((e) => e.is_duplicate).length,
  };
  const colors = { Total: "text-fuchsia-300", Valides: "text-emerald-300", Orphelins: "text-rose-300", Doublons: "text-amber-300" };
  const box = document.querySelector("#counters");
  box.replaceChildren(
    ...Object.entries(c).map(([k, v]) => {
      const d = el("div", "rounded border border-zinc-800 bg-zinc-900 p-2");
      d.append(el("div", `text-2xl font-bold ${colors[k]}`, String(v)), el("div", "text-xs uppercase text-zinc-500", k));
      return d;
    }),
  );
}

function render() {
  const all = [...report.user.entries, ...report.system.entries];
  counters(all);

  for (const key of Object.keys(SCOPES)) {
    const b = document.querySelector(`#tab-${key}`);
    const n = report[key].entries.length;
    b.textContent = `${SCOPES[key].label} · ${n}`;
    b.setAttribute("aria-selected", String(key === active));
    b.className =
      "rounded border px-3 py-1 text-sm " +
      (key === active
        ? "border-fuchsia-400 bg-fuchsia-400/10 text-fuchsia-300"
        : "border-zinc-700 text-zinc-400 hover:border-zinc-500");
  }

  const r = report[active];
  const meta = document.querySelector("#meta");
  meta.textContent = r.error
    ? `Erreur de lecture : ${r.error}`
    : r.value_type
      ? `Type de valeur : ${r.value_type}` + (r.value_type === "REG_SZ" ? " (les %VAR% ne sont pas développées par Windows)" : "")
      : "Valeur Path absente du Registre.";
  meta.className = "mb-3 text-xs " + (r.error ? "text-rose-400" : "text-zinc-500");
  document.querySelector("#entries").replaceChildren(...r.entries.map(row));
}

async function load() {
  try {
    report = await invoke("get_registry_paths");
    render();
  } catch (err) {
    document.querySelector("#meta").textContent = `Erreur : ${err}`;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  for (const key of Object.keys(SCOPES)) {
    document.querySelector(`#tab-${key}`).addEventListener("click", () => {
      active = key;
      if (report) render();
    });
  }
  load();
});
