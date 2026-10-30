# Third-party notices

Most skills in `skills/` are copies of third-party skills. Each section links to the original repository; the MIT License text that covers them is at the end of this file, with each source's copyright line. Keep this repository private unless every license below allows redistribution.

## From Jeffallan/claude-skills (MIT)

Copied from [Jeffallan/claude-skills](https://github.com/Jeffallan/claude-skills) (commit `36f3fe8052da7c57083783268c6ea3265d075265`), under the MIT License, Copyright (c) 2025 Jeffallan.

`api-designer`, `architecture-designer`, `code-documenter`, `cli-developer`, `code-reviewer`, `database-optimizer`, `debugging-wizard`, `devops-engineer`, `django-expert`, `fastapi-expert`, `feature-forge`, `game-developer`, `java-architect`, `javascript-pro`, `monitoring-expert`, `nestjs-expert`, `nextjs-developer`, `pandas-pro`, `playwright-expert`, `postgres-pro`, `python-pro`, `react-expert`, `react-native-expert`, `secure-code-guardian`, `security-reviewer`, `spec-miner`, `spring-boot-engineer`, `sql-pro`, `terraform-engineer`, `test-master`, `the-fool`, `typescript-pro`, `websocket-engineer`.

To update them, copy the folders again from a newer upstream commit and update the commit above.

Modified here: the skills that agents preload (`api-designer`, `architecture-designer`, `code-documenter`, `code-reviewer`, `devops-engineer`, `feature-forge`, `game-developer`, `java-architect`, `monitoring-expert`, `nextjs-developer`, `playwright-expert`, `postgres-pro`, `python-pro`, `react-expert`, `react-native-expert`, `secure-code-guardian`, `security-reviewer`, `spring-boot-engineer`, `sql-pro`, `test-master`, `the-fool`, `typescript-pro`) were trimmed with `tools/trim-preloaded-skill.py`: frontmatter metadata, "When to Use This Skill", "Role Definition" and "Knowledge Reference" sections and the author links removed, and a line added to each output template saying to follow the calling agent's format. Re-run the script after updating them from upstream.

## From wshobson/agents (MIT)

Copied unchanged from [wshobson/agents](https://github.com/wshobson/agents) (`plugins/shell-scripting/skills/`, commit `0874a27b17a22e2b7c2e2bcd725768e0803cad06`), under the MIT License, Copyright (c) 2024 Seth Hobson.

`bash-defensive-patterns`, `shellcheck-configuration`, `bats-testing-patterns`.

## From hmohamed01/powershell-expert (MIT)

`powershell-expert`, copied unchanged from [hmohamed01/powershell-expert](https://github.com/hmohamed01/powershell-expert) (commit `66b07cd612f2fbb6234eefb8cf31d1a33ee7bdf1`). Its README states the MIT License, but the repository has no license file; ask the author for one before redistributing it.

## From github/awesome-copilot (MIT)

`batch-files`, copied unchanged from [github/awesome-copilot](https://github.com/github/awesome-copilot) (`skills/batch-files/`, commit `613efc8c21d9b3ce642dff361d17006cd9967e28`), under the MIT License, Copyright GitHub, Inc.

## From santiagomoneta/3d-printing-skills (MIT)

`klipper-manager` and `orca-slicer`, copied from [santiagomoneta/3d-printing-skills](https://github.com/santiagomoneta/3d-printing-skills) (`skills/`, commit `99fea766b522d8b0a73a3d450e68016498729dd2`). Its README states the MIT License, but the repository has no license file; ask the author for one before redistributing them.

Modified here: `klipper-manager/scripts/upload_config.sh` (backs up before uploading, refuses during a print or when the SAVE_CONFIG block would be lost, takes `--name`) and the upload steps in `klipper-manager/SKILL.md`, which now use it. `orca-slicer` is unchanged.

## From vercel-labs/skills (MIT)

`find-skills`, copied unchanged from [vercel-labs/skills](https://github.com/vercel-labs/skills) (`skills/find-skills/`, commit `a80e4c9d12d607631249d4d7d059f835d4b2f13a`), under the MIT License, Copyright (c) 2026 Vercel, Inc.

## From vercel-labs/agent-browser (Apache 2.0)

`agent-browser`, copied unchanged from [vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser) (`skills/agent-browser/`, commit `372ac5ce20fab885dbf45ee0c9d510ec8c3b6fdb`), under the Apache License 2.0. Its text is at the end of this file.

## From blader/humanizer (MIT)

`humanizer`, copied unchanged from [blader/humanizer](https://github.com/blader/humanizer) (root `SKILL.md` only, commit `8c307f367c91bd974002f10f2be2965d642c13df`, version 3.1.0), under the MIT License, Copyright (c) 2025 Siqi Chen.

## From kepano/obsidian-skills (MIT)

Copied unchanged from [kepano/obsidian-skills](https://github.com/kepano/obsidian-skills) (`skills/`, commit `3ccff5338ea700537839b21900aa5358a0402c98`), under the MIT License, Copyright (c) 2026 Steph Ango (@kepano).

`obsidian-markdown`, `obsidian-bases`, `json-canvas`, `obsidian-cli`, `defuddle`, `knap`.

## Written for this repository

`powershell-scripting`, `batch-scripting`, `add-skill`, `opencode-local`, `opencode-openai`, `project-vault`.

## Copied from the owner's Claude account (2026-10-21)

| Skill | Source and license as stated in the files |
|---|---|
| `ponytail` | `license: MIT` in `SKILL.md` |
| `graphify-codebase-graph` | No license stated; drives the open-source `graphify` CLI (PyPI `graphifyy`) |
| `vercel-react-best-practices`, `vercel-composition-patterns`, `vercel-react-native-skills` | Vercel, `license: MIT` in `SKILL.md` |
| `web-design-guidelines`, `writing-guidelines` | Vercel (`author: vercel`); no license stated |
| `ui-ux-pro-max` | No license file included |
| `frontend-design` | Anthropic, Apache License 2.0 (`frontend-design/LICENSE.txt`) |
| `consolidate-memory` (copied 2026-10-22) | Anthropic; no license stated |

Modified here: `graphify-codebase-graph` was rewritten shorter (about 55% smaller) with the same local-only rules; it now says to grep first for named things, use the graph for structure and impact, and never read `graph.json` directly.

Check the upstream sources before making this repository public or redistributing these copies.

## MIT License text

Applies to every skill above marked MIT, with the copyright line given in its section.

```text
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Apache License 2.0 text

Applies to `agent-browser`. (`frontend-design` keeps its own copy in `frontend-design/LICENSE.txt`.)

```text
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1.  Definitions.

    "License" shall mean the terms and conditions for use, reproduction,
    and distribution as defined by Sections 1 through 9 of this document.

    "Licensor" shall mean the copyright owner or entity authorized by
    the copyright owner that is granting the License.

    "Legal Entity" shall mean the union of the acting entity and all
    other entities that control, are controlled by, or are under common
    control with that entity. For the purposes of this definition,
    "control" means (i) the power, direct or indirect, to cause the
    direction or management of such entity, whether by contract or
    otherwise, or (ii) ownership of fifty percent (50%) or more of the
    outstanding shares, or (iii) beneficial ownership of such entity.

    "You" (or "Your") shall mean an individual or Legal Entity
    exercising permissions granted by this License.

    "Source" form shall mean the preferred form for making modifications,
    including but not limited to software source code, documentation
    source, and configuration files.

    "Object" form shall mean any form resulting from mechanical
    transformation or translation of a Source form, including but
    not limited to compiled object code, generated documentation,
    and conversions to other media types.

    "Work" shall mean the work of authorship, whether in Source or
    Object form, made available under the License, as indicated by a
    copyright notice that is included in or attached to the work
    (an example is provided in the Appendix below).

    "Derivative Works" shall mean any work, whether in Source or Object
    form, that is based on (or derived from) the Work and for which the
    editorial revisions, annotations, elaborations, or other modifications
    represent, as a whole, an original work of authorship. For the purposes
    of this License, Derivative Works shall not include works that remain
    separable from, or merely link (or bind by name) to the interfaces of,
    the Work and Derivative Works thereof.

    "Contribution" shall mean any work of authorship, including
    the original version of the Work and any modifications or additions
    to that Work or Derivative Works thereof, that is intentionally
    submitted to Licensor for inclusion in the Work by the copyright owner
    or by an individual or Legal Entity authorized to submit on behalf of
    the copyright owner. For the purposes of this definition, "submitted"
    means any form of electronic, verbal, or written communication sent
    to the Licensor or its representatives, including but not limited to
    communication on electronic mailing lists, source code control systems,
    and issue tracking systems that are managed by, or on behalf of, the
    Licensor for the purpose of discussing and improving the Work, but
    excluding communication that is conspicuously marked or otherwise
    designated in writing by the copyright owner as "Not a Contribution."

    "Contributor" shall mean Licensor and any individual or Legal Entity
    on behalf of whom a Contribution has been received by Licensor and
    subsequently incorporated within the Work.

2.  Grant of Copyright License. Subject to the terms and conditions of
    this License, each Contributor hereby grants to You a perpetual,
    worldwide, non-exclusive, no-charge, royalty-free, irrevocable
    copyright license to reproduce, prepare Derivative Works of,
    publicly display, publicly perform, sublicense, and distribute the
    Work and such Derivative Works in Source or Object form.

3.  Grant of Patent License. Subject to the terms and conditions of
    this License, each Contributor hereby grants to You a perpetual,
    worldwide, non-exclusive, no-charge, royalty-free, irrevocable
    (except as stated in this section) patent license to make, have made,
    use, offer to sell, sell, import, and otherwise transfer the Work,
    where such license applies only to those patent claims licensable
    by such Contributor that are necessarily infringed by their
    Contribution(s) alone or by combination of their Contribution(s)
    with the Work to which such Contribution(s) was submitted. If You
    institute patent litigation against any entity (including a
    cross-claim or counterclaim in a lawsuit) alleging that the Work
    or a Contribution incorporated within the Work constitutes direct
    or contributory patent infringement, then any patent licenses
    granted to You under this License for that Work shall terminate
    as of the date such litigation is filed.

4.  Redistribution. You may reproduce and distribute copies of the
    Work or Derivative Works thereof in any medium, with or without
    modifications, and in Source or Object form, provided that You
    meet the following conditions:

    (a) You must give any other recipients of the Work or
    Derivative Works a copy of this License; and

    (b) You must cause any modified files to carry prominent notices
    stating that You changed the files; and

    (c) You must retain, in the Source form of any Derivative Works
    that You distribute, all copyright, patent, trademark, and
    attribution notices from the Source form of the Work,
    excluding those notices that do not pertain to any part of
    the Derivative Works; and

    (d) If the Work includes a "NOTICE" text file as part of its
    distribution, then any Derivative Works that You distribute must
    include a readable copy of the attribution notices contained
    within such NOTICE file, excluding those notices that do not
    pertain to any part of the Derivative Works, in at least one
    of the following places: within a NOTICE text file distributed
    as part of the Derivative Works; within the Source form or
    documentation, if provided along with the Derivative Works; or,
    within a display generated by the Derivative Works, if and
    wherever such third-party notices normally appear. The contents
    of the NOTICE file are for informational purposes only and
    do not modify the License. You may add Your own attribution
    notices within Derivative Works that You distribute, alongside
    or as an addendum to the NOTICE text from the Work, provided
    that such additional attribution notices cannot be construed
    as modifying the License.

    You may add Your own copyright statement to Your modifications and
    may provide additional or different license terms and conditions
    for use, reproduction, or distribution of Your modifications, or
    for any such Derivative Works as a whole, provided Your use,
    reproduction, and distribution of the Work otherwise complies with
    the conditions stated in this License.

5.  Submission of Contributions. Unless You explicitly state otherwise,
    any Contribution intentionally submitted for inclusion in the Work
    by You to the Licensor shall be under the terms and conditions of
    this License, without any additional terms or conditions.
    Notwithstanding the above, nothing herein shall supersede or modify
    the terms of any separate license agreement you may have executed
    with Licensor regarding such Contributions.

6.  Trademarks. This License does not grant permission to use the trade
    names, trademarks, service marks, or product names of the Licensor,
    except as required for reasonable and customary use in describing the
    origin of the Work and reproducing the content of the NOTICE file.

7.  Disclaimer of Warranty. Unless required by applicable law or
    agreed to in writing, Licensor provides the Work (and each
    Contributor provides its Contributions) on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
    implied, including, without limitation, any warranties or conditions
    of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
    PARTICULAR PURPOSE. You are solely responsible for determining the
    appropriateness of using or redistributing the Work and assume any
    risks associated with Your exercise of permissions under this License.

8.  Limitation of Liability. In no event and under no legal theory,
    whether in tort (including negligence), contract, or otherwise,
    unless required by applicable law (such as deliberate and grossly
    negligent acts) or agreed to in writing, shall any Contributor be
    liable to You for damages, including any direct, indirect, special,
    incidental, or consequential damages of any character arising as a
    result of this License or out of the use or inability to use the
    Work (including but not limited to damages for loss of goodwill,
    work stoppage, computer failure or malfunction, or any and all
    other commercial damages or losses), even if such Contributor
    has been advised of the possibility of such damages.

9.  Accepting Warranty or Additional Liability. While redistributing
    the Work or Derivative Works thereof, You may choose to offer,
    and charge a fee for, acceptance of support, warranty, indemnity,
    or other liability obligations and/or rights consistent with this
    License. However, in accepting such obligations, You may act only
    on Your own behalf and on Your sole responsibility, not on behalf
    of any other Contributor, and only if You agree to indemnify,
    defend, and hold each Contributor harmless for any liability
    incurred by, or claims asserted against, such Contributor by reason
    of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

Copyright 2025 Vercel Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
