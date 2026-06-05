## When to use

Consult this doc whenever you are:

- Creating a new interactive skill or documentation file
- Deciding whether a piece of knowledge belongs in a skill or a doc
- Onboarding to the project's knowledge architecture

## Two mechanisms

This project has two ways to deliver knowledge to agents:

### Docs (this directory — `docs/`)

Static best-practice documents. Agents read them automatically based on CLAUDE.md directives
("Before creating or modifying X, read `docs/Y.md`"). No explicit invocation needed.

- **Location:** `docs/<name>.md`
- **Loaded when:** CLAUDE.md tells the agent to read them, based on task type
- **Content:** conventions, patterns, rules, anti-patterns — anything an agent should follow
  during implementation
- **Examples:** `docs/superforms.md`, `docs/project-structure.md`, `docs/testing.md`

### Skills (`.agents/skills/`)

Interactive, action-oriented workflows invoked explicitly via `/skill-name`. Skills do things —
they run commands, ask questions, generate artifacts.

- **Location:** `.agents/skills/<skill-name>/SKILL.md`
- **Loaded when:** user invokes `/skill-name` or the agent matches the description
- **Content:** instructions for an interactive workflow, not static reference
- **Examples:** `/grill-me` (interviews the user about a plan), `/to-prd` (generates a PRD),
  `/playwright-cli` (runs Playwright commands)

Skills may be version-controlled externally and installed via `bun x` or other tooling. Docs
should not duplicate or override skill content.

## When to use which

| Situation                                                                      | Use   |
| ------------------------------------------------------------------------------ | ----- |
| Convention that applies whenever code is written (naming, structure, patterns) | Doc   |
| Interactive workflow that runs commands or asks questions                      | Skill |
| Decision tree an agent should follow automatically                             | Doc   |
| Tool the user invokes on demand                                                | Skill |
| Reference material (API patterns, schema placement, test layout)               | Doc   |

**Rule of thumb:** if it's a rule to follow during work → doc. If it's a task to perform on
demand → skill.

## Creating a new doc

1. Create `docs/<name>.md`
2. Start with a `## When to use` section listing trigger conditions
3. Write rules as numbered headings under `## Critical rules`
4. End with a `## References` section linking to relevant external docs
5. Add a CLAUDE.md directive: `- Before creating or modifying <topic>: read \`docs/<name>.md\``

## Creating a new skill

1. Create `.agents/skills/<skill-name>/SKILL.md`
2. Add frontmatter:

```yaml
---
name: skill-name
description: One-line description of what the skill does and when to invoke it.
---
```

3. Write the skill instructions below the frontmatter
4. Optional: add `allowed-tools`, `metadata` fields to the frontmatter
5. Reference files can live in `.agents/skills/<skill-name>/references/`

## Docs are live documents

Docs describe the project's current conventions and patterns. They are not write-once artifacts.
When you use a doc during implementation and notice it is outdated, incomplete, or contradicts
the actual codebase, **update it**. Keeping docs accurate is part of the implementation work, not
a separate task.

## Code examples in docs

Code examples in `.md` files are checked by prettier. All code blocks must pass the linter —
broken examples will fail CI. When writing or updating code examples:

- Use valid TypeScript / Svelte syntax
- Match the project's formatting conventions (run `bun run format` to verify)
- Keep examples minimal but syntactically complete

## Consistency rules

- Docs must not contradict each other. Each concept has one owner doc (see dedup ownership in the
  plan). Other docs reference it with a one-line reminder.
- Docs must not contradict skills. If a skill and a doc disagree, the doc is authoritative for
  conventions and the skill is authoritative for its own workflow.
- All docs and skills must follow the `package-manager.md` rule: **bun only**. Never use `npm`,
  `npx`, `pnpm`, `yarn`, or `deno` in examples (except in "wrong" / "don't" contexts or
  translation tables).
