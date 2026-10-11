---
name: computer-use
description: >-
  OS/window-level inspection and input in visible local app windows through `orcinus computer`:
  native apps and app webviews, plus external browser windows as desktop apps. Not for
  page-only browser automation (use Playwright or CDP for that).
---

# Computer Use

This discovery stub loads the version-matched guide from the Orcinus CLI used for this session.

## Resolve the CLI for this session

Choose the executable once and reuse it for every later command:

- If the `ORCINUS_CLI_COMMAND` environment variable is set, use its value. Orcinus exports
  this for its managed terminals.
- Otherwise, use `orcinus` from `PATH`.

Below, `ORCINUS` is a placeholder for the executable you resolved. Substitute it before
running anything; do not create a shell variable or run `ORCINUS` literally. This works the
same way in POSIX shells, PowerShell, and cmd.exe.

Never fall back to `orca`: it is a different product, and running it here would silently
target a different app with a different runtime. If the resolved executable cannot run,
report its exact error and stop.

## Load the version-matched guide before running Orcinus commands

```text
ORCINUS skills get computer-use
```

Prefer `--json`. Use the selected executable's `--help` for commands or flags the guide does
not cover. If a command reports that Orcinus is not running, start it with `ORCINUS open
--json` and retry. If `skills get` is unknown, explain that updating Orcinus restores the
guide; use `--help` for read-only discovery and do not guess unsupported commands.
