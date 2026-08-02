---
name: grill
description: Grilling session that challenges your plan against this repo's tracked domain model (docs/CONTEXT.md) and decisions (docs/adr/), updating both inline as decisions crystallise. Use when stress-testing a plan against the project's language and documented decisions.
---

<what-to-do>

Interview me relentlessly about every aspect of this plan until we reach a shared understanding. Walk down each branch of the design tree, resolving dependencies between decisions one-by-one. For each question, provide your recommended answer.

Ask the questions one at a time, waiting for feedback on each question before continuing.

If a question can be answered by exploring the codebase, explore the codebase instead.

</what-to-do>

<supporting-info>

## Use this skill, not `grill-with-docs`

The user-level `grill-with-docs` skill keeps the glossary and the ADRs in the parent folder of the
repo, outside any working tree. Monster Master tracks both inside the repo, so that skill's paths
and its "never write these files inside the repo" instruction are wrong here. In this repo, always
use `/grill`.

## Domain awareness

During codebase exploration, also look for existing documentation:

### File structure

The glossary and the ADRs are tracked files in the repo:

```
docs/
├── CONTEXT.md                                        ← the glossary
└── adr/
    ├── 0001-bevy-spacetimedb-with-hand-written-bridge.md
    ├── 0002-stdb-bevy-bridge-architecture.md
    └── ...
```

Tracking them in the repo means they are versioned alongside the code they describe, reviewed in
the same pull request as the change that motivated them, and carried on the current branch rather
than shared by every worktree at once.

Both already exist and hold content, so append to them. Never create a second glossary elsewhere in
the tree, and never split `docs/CONTEXT.md` into per-directory glossaries: this repo is one context
spanning all four crates.

### Writing to them is writing to the branch

Anything this skill writes lands in the working tree, so the project's usual rules apply:

- It appears in `git status` and belongs in the pull request that motivated the decision.
- Per CLAUDE.md, it is not committed until the developer has reviewed it.
- Other worktrees see it only once the branch merges.

## During the session

### Challenge against the glossary

When the user uses a term that conflicts with the existing language in `docs/CONTEXT.md`, call it out immediately. "Your glossary defines 'cancellation' as X, but you seem to mean Y — which is it?"

### Sharpen fuzzy language

When the user uses vague or overloaded terms, propose a precise canonical term. "You're saying 'account' — do you mean the Customer or the User? Those are different things."

### Discuss concrete scenarios

When domain relationships are being discussed, stress-test them with specific scenarios. Invent scenarios that probe edge cases and force the user to be precise about the boundaries between concepts.

### Cross-reference with code

When the user states how something works, check whether the code agrees. If you find a contradiction, surface it: "Your code cancels entire Orders, but you just said partial cancellation is possible — which is right?"

### Update docs/CONTEXT.md inline

When a term is resolved, update `docs/CONTEXT.md` right there. Don't batch these up — capture them as they happen. Use the format in [CONTEXT-FORMAT.md](./references/CONTEXT-FORMAT.md).

`docs/CONTEXT.md` should be totally devoid of implementation details. Do not treat it as a spec, a scratch pad, or a repository for implementation decisions. It is a glossary and nothing else.

### Offer ADRs sparingly

Only offer to create an ADR when all three are true:

1. **Hard to reverse** — the cost of changing your mind later is meaningful
2. **Surprising without context** — a future reader will wonder "why did they do it this way?"
3. **The result of a real trade-off** — there were genuine alternatives and you picked one for specific reasons

If any of the three is missing, skip the ADR. Use the format in [ADR-FORMAT.md](./references/ADR-FORMAT.md).

</supporting-info>
