# Worktree management via worktrunk, wrapped in just

We manage git worktrees with [worktrunk](https://github.com/max-sixty/worktrunk) (`wt`), installed
and pinned through mise, but we keep the existing `just wk::*` recipes as the command surface: the
recipes call `wt` for the git plumbing and layer on our conventions. We deliberately do **not** add
a `.config/wt.toml` with lifecycle hooks.

This is the mirror image of ADR 0001, where we chose to hand-write the Bridge rather than adopt a
crate. The calculus is opposite here on purpose: the Bridge is core, product-differentiating, and
constrained by our day-one web target, so owning it is worth the cost. Worktree management is
undifferentiated developer plumbing where a maintained tool is strictly better than our own.

## Context

The previous `wk.just` derived a worktree's path from its branch name (`../<branch>`). That
assumption broke the moment a worktree's directory name diverged from its branch: a worktree
directory `bump_spacetimesdk` holding branch `docs/stdb-publish-403-ownership` made `just wk::rm`
construct a path that pointed nowhere, and removal failed with "is not a working tree". Branch names
containing `/` had the same fragility. Fixing this by hand means re-implementing worktree-by-branch
tracking, which is exactly what worktrunk already does.

## Considered Options

- **Keep and patch the hand-written `wk.just`.** Rejected. Resolving a worktree by branch (instead
  of guessing its path), merge-aware branch deletion, and safe removal are non-trivial and ongoing
  to maintain, for zero product value.
- **Adopt worktrunk and use `wt` directly as the interface.** Rejected for the committed surface.
  It ties everyone's muscle memory (and any future scripts) to one tool's exact CLI, and scatters
  our conventions (fetch `origin/main` first, the zellij dev layout, docker teardown) across shell
  history instead of one place.
- **Adopt worktrunk, wrap it in `just` (chosen).** `wt` is the engine; `just wk::*` is the stable,
  centralized interface. If we ever drop or swap `wt`, only the recipe bodies change and the
  commands stay. The conventions live in one file.

We also chose **explicit recipes over worktrunk lifecycle hooks** (no `.config/wt.toml`). Things run
when a recipe calls them, not implicitly on create/remove. This keeps behavior visible in the
recipe rather than in a separate hook config.

## Consequences

- The path-guessing bug class is gone. Recipes that need a worktree's location ask git
  (`git worktree list --porcelain`), which is authoritative, and `wt` itself never guesses.
- `wt` is pinned to an explicit release tag in `.mise/config.toml` and locked in `.mise/mise.lock`.
  Not `"latest"`: worktrunk's binary self-reports a version decoupled from its release tags, and
  mise derives the locked version from the binary, so `"latest"` silently resolves to a stale
  release. Bump the tag to upgrade.
- The worktree directory layout is a per-developer worktrunk setting (it lives in user config, not
  project config, by worktrunk's design), so it is not committed. We accept the default layout
  (`../main.<branch>`). Naming is now cosmetic, since worktrees are tracked by branch.
- There is no `just wk::switch`. A `just` recipe runs in a child process and cannot change the
  parent shell's directory, which is the whole point of switching. Navigate between worktrees with
  zellij tabs, or call `wt switch <branch>` directly (its shell integration does the cd).
- worktrunk's shell integration (`eval "$(wt config shell init …)"`) stays in each developer's
  personal config, because it is shell-specific.
- The decision is cheap to reverse: rewrite the recipe bodies and drop the tool, and the
  `just wk::*` commands keep working.
