---
name: "graphify-codebase-graph"
description: "Use for questions about a codebase's structure, architecture, file relationships or the impact of a change - map a project into a queryable knowledge graph with the graphify CLI, kept fully local (no cloud). For a named symbol, string or file, Grep is cheaper."
---

# Graphify: local codebase graph

`graphify` (PyPI `graphifyy`) parses code locally with tree-sitter into a graph of files, symbols and their relations.

## When to use it

- **Grep first** for anything with a name: a symbol, string, file, config key or error message. It is cheaper, and in measured runs the graph cost 30-70% more tokens on such questions with no better answer.
- **The graph** for what grep answers badly: what depends on X (`affected`), how A reaches B (`path`), the hubs or architecture of an unfamiliar area (`GRAPH_REPORT.md`, `god-nodes`).
- **Small project (under ~20k lines of code): no graph for architecture either.** Read the folder structure (`git ls-files`) and the main files instead; it was cheaper in measured runs with the same answer. Impact and reach questions (`affected`, `path`) can still use it. Size check, code files only:

  ```bash
  git ls-files -z -- '*.py' '*.js' '*.jsx' '*.ts' '*.tsx' '*.go' '*.rs' '*.java' '*.kt' '*.c' '*.h' '*.cpp' '*.cs' '*.rb' '*.php' '*.swift' '*.sql' '*.sh' | xargs -0 wc -l | tail -1
  ```

## Local only

- `GRAPHIFY_BACKEND=none` on every command (code only, nothing sent). `GRAPHIFY_BACKEND=ollama` only if docs/PDFs/images need a semantic pass and a local Ollama runs.
- Never use Graphify Cloud, set or ask for cloud LLM API keys, or push to a non-localhost graph DB (`--neo4j-push`, `--falkordb-push`). If a cloud key is already set and the backend is not forced, warn and stop until the user confirms.

## Use

1. `graphify --version`. Missing: do not install; ask the user to run `uv tool install graphifyy` (or `pipx install graphifyy`) and use Grep/Glob meanwhile.
2. Graph: reuse `graphify-out/`, refreshing it with `graphify . --update` after code changes. No graph and a structure question worth it: `graphify extract . --code-only`, then `graphify cluster-only . --no-label --no-viz` for `GRAPH_REPORT.md`.
3. Ask, cheapest first, and open only the lines the answer points to (`src=<file> loc=L<n>`):
   - `graphify affected "<symbol>"`, `graphify path "A" "B"`, `graphify explain "<symbol>"`, `graphify god-nodes`
   - `graphify query "<question>" --budget 1500` (raise the budget only if it says TRUNCATED and the answer is missing)
   - `GRAPH_REPORT.md` for an architecture overview: read its head, not the whole file, on big projects.
   - **Never read `graph.json` directly:** it is huge (1.6 MB on a 10k-line project).
4. Edges are `EXTRACTED` (in the source) or `INFERRED`; say which when it matters. Flags differ by version: `graphify --help` is the source of truth.

Never commit `graphify-out/`.
