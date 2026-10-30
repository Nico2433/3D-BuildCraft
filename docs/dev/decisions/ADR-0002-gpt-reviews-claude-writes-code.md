# ADR-0002 GPT reviews and tests, Claude writes code

## Context

One model writing and checking its own code misses its own blind spots. The initial workflow used separate implementation, review and testing agents.

## Decision

Claude writes the code. GPT, through the `openai-reviewer` and `openai-tester` agents run via OpenCode, reviews it and writes tests. The packs were installed into the repo's `.claude/` in `b10d3c3`; `CLAUDE.md` holds the review, memory and language rules. Decided 2026-10-27.

## Alternatives considered

- Claude writes, reviews and tests alone: cheaper, but no independent check.
- GPT writes the code: Claude was selected for implementation.

## Consequences

- The GPT review found real bugs (the fixes are in `1aa89d0` and `61967aa`); some findings are still open, listed in [Architecture](../architecture.md).
- Tests written by GPT need a Claude pass for style and clippy.
- Agent-specific instructions depend on the contributor’s tooling; the checks and language rules remain in `CLAUDE.md`.
