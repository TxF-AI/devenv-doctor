import "./styles.css";

const { invoke } = window.__TAURI__.core;

const TAG = "ml-2 rounded px-2 py-0.5 text-xs font-bold uppercase";

function row(e) {
  const li = document.createElement("li");
  const ok = e.exists;
  li.className =
    "flex items-center justify-between rounded border px-3 py-2 break-all " +
    (ok
      ? "border-emerald-500/40 bg-emerald-500/5 text-emerald-300"
      : "border-rose-500 bg-rose-500/10 text-rose-300 shadow-[0_0_10px_rgba(244,63,94,.5)]");

  const label = document.createElement("span");
  label.textContent = e.path; // textContent : jamais d'HTML injecté depuis le PATH
  li.append(label);

  const tags = document.createElement("span");
  tags.className = "shrink-0";
  if (!ok) tags.append(tag("Orphelin", "bg-rose-500 text-zinc-950"));
  if (e.is_duplicate) tags.append(tag("Doublon", "bg-amber-400 text-zinc-950"));
  li.append(tags);
  return li;
}

function tag(text, cls) {
  const s = document.createElement("span");
  s.className = `${TAG} ${cls}`;
  s.textContent = text;
  return s;
}

async function load() {
  const list = document.querySelector("#entries");
  const summary = document.querySelector("#summary");
  try {
    const entries = await invoke("get_path_entries");
    list.replaceChildren(...entries.map(row));
    const orphans = entries.filter((e) => !e.exists).length;
    const dups = entries.filter((e) => e.is_duplicate).length;
    summary.textContent = `${entries.length} entrées · ${orphans} orphelines · ${dups} doublons`;
  } catch (err) {
    summary.textContent = `Erreur : ${err}`;
  }
}

window.addEventListener("DOMContentLoaded", load);
