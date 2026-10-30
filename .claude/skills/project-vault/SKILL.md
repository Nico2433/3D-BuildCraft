---
name: project-vault
description: Keep each project's knowledge in the person's Obsidian vault - a folder per project with its graphify code graph and its documentation (overview, architecture, decisions, specs, progress log) in one fixed format - and read it first to save tokens. Use at the start of work on a project to orient (vault index, then graphify query, then only the files that matter), and after a change lands to update the graph and the docs. Also use when the person asks to document a project, record a decision or set up the vault.
---

# Project vault: graphify + Obsidian

The vault is the project's memory between sessions. Reading a short index costs a few hundred tokens; re-reading the code to rediscover the same facts costs thousands. So: **vault first, then a targeted search, code last.**

## 1. Find the vault

The vault path lives in `~/.config/agents-skills/config.json`:

```json
{ "obsidianVault": "/path/to/vault" }
```

- Read it (Read tool or `cat`). If the folder exists, use it.
- **Not configured, and you are the main session:** ask the person where their vault is. Offer the vaults Obsidian knows about (the `path` values in `~/.config/obsidian/obsidian.json`; on macOS `~/Library/Application Support/obsidian/obsidian.json`), plus "other". Once they answer, write the config file (`mkdir -p ~/.config/agents-skills` first, keep any other keys in it) and save the path in your auto-memory too, so it is never asked again.
- **Not configured, and you are a subagent:** you cannot ask. Stop using this skill, do the task without the vault, and say in your report that the vault is not configured so the main session asks.
- Never create a new vault, and never write outside `<vault>/Projects/`.

## 2. Find the project folder

Every project lives in `<vault>/Projects/<slug>/`. `<slug>` is the repository folder name in lowercase kebab-case (`GPS Fleet` → `gps-fleet`).

- First look for an existing index whose `repo:` property matches this repository's path: `grep -l "^repo: $(git rev-parse --show-toplevel)$" <vault>/Projects/*/*.md`. That folder wins even if its name differs.
- If there is none, create the folder from the templates in `references/templates.md`: the index note `<slug>.md`, the base `<slug>.base`, and the folders `docs/`, `decisions/`, `log/` and `graph/`. Only create the docs you have content for; never fill a note with placeholders.

```text
Projects/<slug>/
  <slug>.md          index: what the project is, stack, commands, links to every doc
  <slug>.base        Bases table of every note in the project, by type and status
  docs/              overview.md, architecture.md, data-model.md, specs/<feature>.md
  decisions/         ADR-0001-<title>.md, numbered, never renumbered
  log/               YYYY-MM-DD.md, one note per working day
  graph/             graphify output: GRAPH_REPORT.md, graph notes, graph.canvas
```

## 3. Orient before reading code (saves tokens)

1. Read `<slug>.md`. It is short by design and links to everything else.
2. Read only the docs it links to that touch the task (often one).
3. Search for what has a name (a symbol, string, file, config key) with Grep and Glob: it is the cheapest path. Use the graph only for what grep answers badly: what a change affects (`graphify affected "<symbol>"`), how A reaches B (`graphify path "A" "B"`), the hubs of an unfamiliar area (`graphify god-nodes`, or the vault's `graph/GRAPH_REPORT.md`). On a project under ~20k lines of code, skip the graph for architecture too and read the folder structure and the main files. Answers give `src=<file> loc=L<line>`. Never read `graph.json` directly.
4. Open only the files and lines the search pointed to.

When you do use the graph and `graphify-out/graph.json` is missing or older than `HEAD` (compare the commit in `GRAPH_REPORT.md` with `git rev-parse --short HEAD`), refresh it first (step 4). If the `graphify` CLI is not installed, do not install it: say so and use Grep and Glob.

## 4. Refresh the graph

Run from the repository root. Code is parsed locally; `GRAPHIFY_BACKEND=none` keeps everything off the network (see the graphify-codebase-graph skill for the rules).

```bash
GRAPHIFY_BACKEND=none graphify update .                       # or the first time: graphify extract . --code-only
GRAPHIFY_BACKEND=none graphify cluster-only . --no-label --no-viz   # writes graphify-out/GRAPH_REPORT.md
GRAPHIFY_BACKEND=none graphify export obsidian --graph graphify-out/graph.json --dir "<vault>/Projects/<slug>/graph"
cp graphify-out/GRAPH_REPORT.md "<vault>/Projects/<slug>/graph/GRAPH_REPORT.md"
```

- The export also writes a `graph/.obsidian/` folder meant for opening it as its own vault. Remove just that folder (`rm -r "<vault>/Projects/<slug>/graph/.obsidian"`), so it does not sit inside the person's vault as a second config.
- `graphify-out/` stays in the repository (it holds `graph.json` for queries) and is never committed: check it is in `.gitignore`.
- Graph notes are generated: never edit them by hand, and never link to them from docs except `[[Projects/<slug>/graph/GRAPH_REPORT|Graph report]]`.
- Update the index's `graph_commit:` property to the commit you built from.

## 5. Write and update the docs

Follow `references/templates.md` exactly: same frontmatter, same headings, same order, for every project. That sameness is what lets a later session find things without reading everything.

- **When:** after a change lands (new feature, schema change, new command, a decision), not for every edit. Add a line to today's `log/` note for each session that changed something.
- **Links:** always use path-qualified wikilinks inside the vault, `[[Projects/<slug>/docs/architecture|Architecture]]`, never bare `[[architecture]]`: many projects have notes with the same name.
- **One source of truth:** the repository keeps its own README, setup and API docs; the vault links to them by path instead of copying them. The vault holds what the repo does not: the map, the reasons and the history.
- **Short and current:** fix or delete stale text instead of adding a contradicting note. Update `updated:` on every note you change.
- Follow the obsidian-markdown skill for syntax (properties, callouts, embeds) and obsidian-bases for the `.base` file.
- Never put secrets, tokens, `.env` contents or customer data in the vault.

## Report

Say which vault notes you read or changed (path-qualified), whether the graph was refreshed and from which commit, and anything you could not do (no vault configured, no graphify CLI).
