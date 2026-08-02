## When to use

Consult this doc whenever you are:

- Creating a new interactive skill or documentation file
- Deciding whether a piece of knowledge belongs in a skill or a doc
- Onboarding to the project's knowledge architecture

## Kinds of knowledge

This project keeps agent-facing knowledge in four places. Two are mechanisms (docs, skills), two are
artifacts with a single owner each (the glossary, the decision records).

### Docs (this directory — `docs/`)

Static best-practice documents. Agents read them automatically based on CLAUDE.md directives
("Before creating or modifying X, read `docs/Y.md`"). No explicit invocation needed.

- **Location:** `docs/<name>.md`
- **Loaded when:** CLAUDE.md tells the agent to read them, based on task type
- **Content:** conventions, patterns, rules, anti-patterns — anything an agent should follow
  during implementation
- **Examples:** `docs/spacetimedb.md`, `docs/cicd.md`

### Skills (`.agents/skills/`)

Interactive, action-oriented workflows invoked explicitly via `/skill-name`. Skills do things —
they run commands, ask questions, generate artifacts.

- **Location:** `.agents/skills/<skill-name>/SKILL.md`
- **Loaded when:** user invokes `/skill-name` or the agent matches the description
- **Content:** instructions for an interactive workflow, not static reference
- **Examples:** `/grill` (interviews the user about a plan and updates the glossary and ADRs
  inline), `/to-prd` (generates a PRD), `/to-issues` (splits work into issues)

Skills may be version-controlled externally and installed via npx skills or other tooling. Docs
should not duplicate or override skill content.

### Glossary (`docs/CONTEXT.md`)

The project's ubiquitous language: one canonical name per concept, plus the aliases to avoid. It is
the only doc CLAUDE.md marks as always-read, because naming happens in every task.

- **Location:** `docs/CONTEXT.md`. One glossary only: this repo is a single context spanning all
  four crates, so it is never split per crate or per directory
- **Loaded when:** always, per CLAUDE.md Required Reading
- **Content:** terms, relationships, flagged ambiguities. No implementation details, no decisions,
  no rules
- **Written by:** `/grill`, inline, as terms get resolved. Format lives in
  `.agents/skills/grill/references/CONTEXT-FORMAT.md`

### Decision records (`docs/adr/`)

Numbered records of decisions that were hard to reverse, surprising without context, and the result
of a real trade-off. An ADR captures why a choice was made at a point in time, not how the code
behaves today.

- **Location:** `docs/adr/NNNN-slug.md`, sequentially numbered
- **Loaded when:** an agent needs the reasoning behind an existing decision, or `/grill` is checking
  whether a question is already settled
- **Content:** append-only history. Supersede an old ADR with a new one rather than rewriting it to
  match the present
- **Written by:** `/grill`, on the rare occasion it offers one. Format lives in
  `.agents/skills/grill/references/ADR-FORMAT.md`
- Docs may cite an ADR by path. Code comments may not, per the Comment Rules in CLAUDE.md

## When to use which

| Situation                                                                      | Use   |
| ------------------------------------------------------------------------------ | ----- |
| Convention that applies whenever code is written (naming, structure, patterns) | Doc   |
| Interactive workflow that runs commands or asks questions                      | Skill |
| Decision tree an agent should follow automatically                             | Doc   |
| Tool the user invokes on demand                                                | Skill |
| Reference material (API patterns, schema placement, test layout)               | Doc   |
| Canonical name for a domain concept, or an alias to stop using                 | Glossary |
| Hard-to-reverse choice with real alternatives a reader would question          | ADR   |

**Rule of thumb:** if it's a rule to follow during work → doc. If it's a task to perform on
demand → skill. If it's what to call something → glossary. If it's why a settled choice was
made → ADR.

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

Keep code examples accurate against the current codebase. When writing or updating them:

- Use valid Rust / TOML / shell syntax (the stack is Rust + Bevy + SpacetimeDB)
- Match the project's formatting conventions (`cargo fmt` for Rust)
- Keep examples minimal but syntactically complete

## Consistency rules

- Docs must not contradict each other. Each concept has one owner doc (see dedup ownership in the
  plan). Other docs reference it with a one-line reminder.
- Docs must not contradict skills. If a skill and a doc disagree, the doc is authoritative for
  conventions and the skill is authoritative for its own workflow.
- Tooling conventions: **`cargo`** for the Rust workspace, **`just`** for task running, **`mise`**
  for installing tools, **`spacetimedb-cli`** (via mise) for the Module. Never hand-edit the
  generated `stdb_bindings` — regenerate with `just stdb::generate`.
