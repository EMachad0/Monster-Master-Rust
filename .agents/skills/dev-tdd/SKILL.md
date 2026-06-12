---
name: dev-tdd
description: Collaborative, pair-style test-driven development. The agent writes the tests, the developer writes the implementation, and they move through the task one vertical slice at a time. Use when building a feature or fixing a bug together with the developer, test-first.
---

# Collaborative TDD

A pair-style variant of TDD. The roles are fixed:

- **Agent writes the tests.** Behavior-focused. Never writes production code.
- **Developer writes the implementation.** Owns all production code, including the real interface.

You move through the task **one vertical slice at a time**, and **every piece of work is reviewed**
before moving on.

For *how* to write tests in this repo (layers, fakes, conventions, running them), read
`docs/testing.md`. This skill governs the *workflow*; that doc governs the *mechanics*.

## What a slice is

A **slice = one code piece** — a function, method, or cohesive cluster of behavior. It is the
vertical unit of work and the **red-green unit** (not the individual test). A single slice usually
has **several tests** covering happy and unhappy paths.

The task is built up slice by slice. **Tracer-bullet ordering**: the first slice is the thinnest
vertical that proves the path end-to-end; later slices thicken it.

## Why this diverges from stock TDD

Stock TDD forbids "all tests, then all code" (horizontal slicing) because batched tests describe
*imagined* behavior. Here that risk is gone **within a slice**: the design is already fixed by a
prior grill (see Planning), so a slice's tests describe *known* behavior, not guesses. Batching a
slice's tests is therefore fine.

The horizontal risk only bites **across slices**, so the one hard rule is:

> **Never pre-write tests for a future slice.** Finish slice N completely — tests, implementation,
> green, review, commit — before touching slice N+1's tests.

## Planning (before any slice)

1. **Assume the design is settled by a prior grill.** If it is not — the behavior is fuzzy or
   undecided — say so and recommend running `/grill-with-docs` first. Do **not** slice blind.
2. **Propose the slice breakdown**: an ordered list of code pieces, first slice = tracer bullet.
3. **Wait for the developer to approve or reorder** the breakdown before starting.

## The per-slice loop

For each slice, in order:

1. **Propose the test list** — the behavior each test will pin, happy **and** unhappy paths listed
   explicitly so unhappy paths can't be silently dropped. → *Wait for the developer to approve the
   list.*
2. **Write the tests** — tests only. They are behavior-focused and may not compile yet. The
   interface they assume is a **proposal**, not binding. → *Wait for the developer to review the
   tests.*
3. **Developer implements** all production code and decides the real interface (it may diverge from
   what the tests assumed).
4. **Reconcile** the tests to the developer's actual interface — the developer edits them, or asks
   you to adapt them — until they compile and pass.
5. **Review the developer's code** once green:
   - **Bug found** → write a new test pinning the bug. If no test can express it, tell the developer
     in plain text.
   - **Refactor / rename / typo ideas, however small** → tell the developer; **the developer decides**
     whether to apply them. Do not apply them yourself.
6. **Close the slice** — once you fully agree (all raised issues resolved) **or** the developer says
   to proceed: **commit** the slice, then move to the next one.

## Rules

- Write tests, never production code. Refactors are *suggestions* for the developer to apply.
- One behavior per test; no cramming multiple behaviors into one test.
- Every slice's test list must cover happy **and** unhappy paths explicitly.
- Stop and wait at each review checkpoint (test list, tests, post-green review); proceed only on the
  developer's go.
- Never pre-write tests for a future slice.
- Commit only after the slice has been reviewed (see the commit rules in CLAUDE.md).

## Checklist per slice

```
[ ] Test list proposed (happy + unhappy) and approved
[ ] Tests written — behavior-focused, one behavior each
[ ] Developer implemented; tests reconciled to the real interface
[ ] Tests green
[ ] Code reviewed; bugs pinned with tests, refactors offered to the developer
[ ] Slice committed; on to the next
```
