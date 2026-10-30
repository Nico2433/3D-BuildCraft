# Project vault templates

Every note starts with these properties. Keep the order. Dates are `YYYY-MM-DD`.

```yaml
---
type: index | overview | architecture | data-model | spec | decision | log | graph
project: <slug>
status: active | draft | accepted | superseded | done | archived
created: 2026-10-26
updated: 2026-10-26
tags:
  - project/<slug>
---
```

Write the notes in English (see the agents' language rules). Headings below are fixed: keep them, in this order, and drop a section only when it truly does not apply.

## Index: `Projects/<slug>/<slug>.md`

```markdown
---
type: index
project: <slug>
status: active
repo: /absolute/path/to/repository
stack: [react, supabase]
graph_commit: abc1234
created: 2026-10-26
updated: 2026-10-26
tags:
  - project/<slug>
---

# <Project name>

<One or two sentences: what it is and who it is for.>

## Stack and commands

| What | Command |
|---|---|
| Install | `pnpm install` |
| Dev | `pnpm dev` |
| Test | `pnpm test` |

## Map

- [[Projects/<slug>/docs/overview|Overview]]
- [[Projects/<slug>/docs/architecture|Architecture]]
- [[Projects/<slug>/docs/data-model|Data model]]
- [[Projects/<slug>/graph/GRAPH_REPORT|Graph report]] (commit `abc1234`)

## Specs

- [[Projects/<slug>/docs/specs/<feature>|<Feature>]] - <status>

## Decisions

- [[Projects/<slug>/decisions/ADR-0001-<title>|ADR-0001 <Title>]] - accepted

## Current state

<Three to five lines: what works, what is in progress, what is next. Link today's log.>

## All notes

![[Projects/<slug>/<slug>.base]]
```

Keep the index under about 60 lines: it is read at the start of every session.

## Overview: `docs/overview.md`

```markdown
# Overview

## Problem
## Users
## Scope
In scope / out of scope, as two short lists.
## Repository docs
Links by path to the repo's README, setup and API docs (not copies).
```

## Architecture: `docs/architecture.md`

```markdown
# Architecture

## Components
One line per component: name, responsibility, main folder (`src/...`).
## Data flow
A short numbered list or a Mermaid diagram of the main request path.
## Integrations
External services, what for, and where the code that calls them lives.
## Hubs
The most connected symbols from the graph report, and why they matter.
## Risks and constraints
```

## Data model: `docs/data-model.md`

```markdown
# Data model

## Entities
Table: entity, key fields, owner, notes.
## Relationships
## Access rules
Who can read and write what (RLS, roles).
## Migrations
Where they live and the naming rule.
```

## Spec: `docs/specs/<feature>.md`

```markdown
---
type: spec
status: draft | active | done
---

# <Feature>

## Goal
## Acceptance criteria
Numbered, each one testable.
## Design
## Open questions
## Changes
Links to the PRs or commits that implemented it.
```

## Decision: `decisions/ADR-NNNN-<kebab-title>.md`

Number with four digits, the next free one. Never renumber or delete; a replaced decision gets `status: superseded` and a link to the new one.

```markdown
---
type: decision
status: accepted | superseded
supersedes: ""
---

# ADR-NNNN <Title>

## Context
## Decision
## Alternatives considered
One line each, with why it was rejected.
## Consequences
```

## Log: `log/YYYY-MM-DD.md`

```markdown
---
type: log
---

# YYYY-MM-DD

- <What changed, one line each, with a link to the PR or commit.>

## Next
- <What is left.>
```

## Base: `Projects/<slug>/<slug>.base`

```yaml
filters:
  and:
    - 'file.inFolder("Projects/<slug>")'
    - '!file.inFolder("Projects/<slug>/graph")'
    - 'file.ext == "md"'
views:
  - type: table
    name: "By type"
    groupBy:
      property: type
      direction: ASC
    order:
      - file.name
      - status
      - updated
  - type: table
    name: "Open specs"
    filters:
      and:
        - 'type == "spec"'
        - 'status != "done"'
    order:
      - file.name
      - status
      - updated
```


The second filter hides the generated graph notes; the graph report is linked from the index instead.
