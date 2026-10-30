---
name: add-skill
description: How to bring a new skill into the owner's agents-skills repository after finding it - review it, copy it unchanged, record its source, add it to a pack and test the install. Use together with find-skills whenever the person wants to add, save or install a skill found online, instead of installing it globally with npx skills add.
---

# Adding a skill

Use `find-skills` (or a web search) to find candidates. Do not run `npx skills add ... -g -y`: it installs into the user folder with no review, outside the repository every project installs from. Bring the skill into `Nico2433/agents-skills` instead.

## 1. Review before copying

Read the whole skill folder, not only `SKILL.md`, and report:

- **Source:** the original repository, path and commit. Prefer the upstream repository over aggregator or marketplace copies.
- **License:** from the repository's license file. If only a README badge or nothing states it, say so.
- **Overlap:** whether an installed skill or a built-in tool already does this. Skip it if it adds little.
- **Requirements:** CLIs, packages, accounts, API keys, paid services. Agents never install tools on their own, so a skill whose CLI is missing only falls back or fails.
- **Risks:** commands that send data to third parties, use the person's cookies or tokens, run code downloaded at runtime, or cost money.
- **Description:** a "MUST USE" or very broad description makes it fire on unrelated tasks and compete with other skills. Point it out.

Recommend add, add to an optional pack, or skip, and wait for the person's decision.

## 2. Copy it

- Copy the folder unchanged into `skills/<name>/`. Resolve symlinks (`cp -rL`).
- If a change is needed, keep it small and list it under the source's section in `THIRD_PARTY_NOTICES.md` ("Modified here: ...").
- Do not add per-skill license files. Record the source in `THIRD_PARTY_NOTICES.md`: link to the original repository, path, commit, license and copyright line. The MIT text at the end of that file covers MIT skills; add the full text of any other license there too.

## 3. Add it to a pack

- Add the name to the `skills` list of the right pack in `packs/`, or create a pack (see the README, "Maintain").
- Preload it into an agent (`preload`) only if the role needs it on almost every task; preloaded skills cost tokens on every run.
- Update the pack table in `README.md`.

## 4. Test and propose

- Run `node install.mjs <pack> --target <temp dir>` and check the skill folder and `THIRD_PARTY_NOTICES.md` are installed.
- Open a pull request with what the skill does, its source and license, and the review findings.
- Projects get it the next time they reinstall their packs.
