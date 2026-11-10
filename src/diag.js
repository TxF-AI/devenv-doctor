// « Copy AI Diagnostic Prompt » : résumé Markdown de l'état du PATH, prêt à coller dans un assistant IA
// (Claude Code, Cursor…). Toujours en anglais ; le nom d'utilisateur Windows est masqué dans les chemins.

const MAX_LINES = 40;
const pad = (n) => String(n).padStart(2, "0");
const stamp = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
/** Masque le nom du profil (`C:\Users\<nom>`), comme le fait l'export dotfiles. */
export const anonymize = (p) => String(p).replace(/^([A-Za-z]:\\Users\\)[^\\]+/i, "$1<user>");
const code = (v) => `\`${anonymize(v).replace(/`/g, "'")}\``;
const hive = (scope) => (scope === "System" ? "HKLM" : "HKCU");

function capped(lines) {
  if (!lines.length) return ["- None"];
  const more = lines.length - MAX_LINES;
  return more > 0 ? [...lines.slice(0, MAX_LINES), `- … and ${more} more`] : lines;
}

/**
 * Fonction pure, testable sans l'interface.
 * entries : entrées dans l'ordre de résolution (n, scope, raw_path, expanded_path, exists, is_duplicate) ;
 * conflicts : EntryConflicts[] du backend ; lengths : LengthReport ou null ; numberOf(scope, index) → n affiché.
 */
export function buildPrompt({ version, isAdmin, entries, lengths, conflicts, numberOf, now }) {
  const line = (e) => `- #${e.n} [${hive(e.scope)}] ${code(e.raw_path)}${e.expanded_path && e.expanded_path !== e.raw_path ? ` → ${code(e.expanded_path)}` : ""}`;
  const orphans = entries.filter((e) => !e.exists);
  const dups = entries.filter((e) => e.is_duplicate);
  const byN = new Map(entries.map((e) => [e.n, e.raw_path]));
  const ref = (scope, index) => {
    const n = numberOf(scope, index);
    return `#${n} ${code(byN.get(n) ?? "?")}`;
  };
  const shadowed = conflicts.flatMap((c) =>
    c.names.filter((x) => !x.wins).map((x) => `- ${code(x.name)}: ${ref(c.scope, c.index)} is shadowed by ${ref(x.other_scope, x.other_index)} (Windows runs that one)`),
  );
  const len = (label, i) => `- **${label}:** ${i.length} characters (risk: ${i.risk.level})`;
  const lens = lengths
    ? [len("User PATH (HKCU)", lengths.user), len("System PATH (HKLM)", lengths.system), len("Effective PATH", lengths.combined)]
    : ["- Not available"];
  return `### DevEnv Doctor Diagnostic Summary

**Context:**
- **Application:** DevEnv Doctor v${version}
- **OS:** Windows
- **Timestamp:** ${stamp(now)}
- **Elevated Privileges (Admin):** ${isAdmin ? "Yes" : "No"}

#### PATH Length (thresholds: 1024 for \`setx\`, 2048, 8192)
${lens.join("\n")}

#### PATH Health
- **Entries:** ${entries.length} (${entries.filter((e) => e.scope === "System").length} in HKLM, ${entries.filter((e) => e.scope === "User").length} in HKCU; Windows resolves HKLM first)
- **Broken entries (folder missing):** ${orphans.length}
- **Duplicate entries:** ${dups.length}
- **Shadowed executables:** ${shadowed.length}

#### Broken Entries
${capped(orphans.map(line)).join("\n")}

#### Duplicate Entries
${capped(dups.map(line)).join("\n")}

#### Shadowed Executables (version conflicts)
${capped(shadowed).join("\n")}

#### Task for AI Assistant
Analyze the diagnostic data above and provide:
1. Which entries are safe to remove, and which ones I should keep or repair (for example a tool that is not installed yet).
2. For each shadowed executable, whether the winning entry is the version I most likely want, and how to reorder the PATH if it is not.
3. Whether the PATH length is a risk, and how to shorten it without breaking tools.
`;
}
