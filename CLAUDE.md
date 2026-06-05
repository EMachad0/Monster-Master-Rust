# Fleet

## Tech Stack

TODO

## Required Reading

Before starting work, read any docs that match the task at hand:

- Before creating or modifying CI/CD workflows: read `docs/cicd.md`

## Shell Rules

- **Never use `find -exec`**. It triggers a permission prompt that cannot be auto-allowed. Use one of these alternatives:
  - `find ... -print0 | xargs -0 command` (pipe to xargs)
  - `fd` (already in the allowed list, modern alternative to find)

## Agent Rules

- **Never use the `AskUserQuestion` tool.** If you need clarification, state your assumption and proceed. If you need to present options, list them in plain text output instead.
