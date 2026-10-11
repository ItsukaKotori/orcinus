---
name: computer-use
description: >-
  OS/window-level inspection and input in visible local app windows through `orcinus computer`:
  native apps and app webviews, plus external browser windows as desktop apps. Not for
  page-only browser automation (use Playwright or CDP for that).
---

# Computer Use

Use this skill for desktop UI through `orcinus computer`. For a website or web app, use it only when the page is in an external desktop browser window that needs desktop-level control; for page-only automation, use a page-automation tool such as Playwright or CDP. This CLI talks to the Orcinus app on macOS (Darwin 14+); `computer *` commands are unavailable elsewhere.

## Preconditions

- `ORCINUS` below is a placeholder for the executable you resolved in the stub (`ORCINUS_CLI_COMMAND` or `orcinus` on `PATH`); substitute it before running. Never fall back to `orca`, a different product.
- `orcinus status --json` reports whether the runtime is reachable; `orcinus open --json` launches the app and waits. Both return without waiting for a desktop window in `status`.
- Prefer `--json`; see Screenshots below for image output.
- `--worktree` / `--session` are accepted only as explicit pass-through values; this build never resolves an active worktree or session implicitly. Omit them unless you have an exact value to forward.
- Do not push, submit forms, send messages, buy items, delete data, change account settings, or expose secrets unless the user explicitly asked for that action.
- If an app contains sensitive content, read only what the user requested.

```text
ORCINUS computer capabilities --json
```

## Core Loop

```text
ORCINUS computer list-apps --json
ORCINUS computer get-app-state --app com.spotify.client --json
ORCINUS computer click --app com.spotify.client --element-index 42 --json
```

Use the fresh state returned by each action for the next element index. Element indexes are the numeric labels shown in the tree; they may be sparse when noisy sections are omitted, so never infer valid indexes from `elementCount` or "Visible elements." Element indexes are short-lived and go stale after delays, navigation, focus changes, scrolling, window changes, or app re-rendering.

In `--json` output, read the accessibility tree and action indexes from `result.snapshot.treeText`; `elementCount` is only a count and must not be used to infer indexes.

## App Selectors

Prefer bundle IDs from `list-apps`; names are acceptable when unambiguous. Use `pid:<number>` only when bundle ID or name matching is ambiguous.

```text
ORCINUS computer get-app-state --app com.microsoft.edgemac --json
ORCINUS computer get-app-state --app Spotify --json
ORCINUS computer get-app-state --app pid:12345 --json
```

For apps with multiple windows or ambiguous titles, run `list-windows` first. Prefer `--window-id <id>` when the listed id is not `none`; otherwise use `--window-index <n>`. Once you choose a window, pass the same selector to `get-app-state` and later actions until the target window changes.

## Commands

```text
ORCINUS computer permissions --json
ORCINUS computer capabilities --json
ORCINUS computer list-apps --json
ORCINUS computer list-windows --app <app> --json
ORCINUS computer get-app-state --app <app> --json
ORCINUS computer get-app-state --app <app> --restore-window --json
ORCINUS computer click --app <app> --element-index <index> --json
ORCINUS computer click --app <app> --x 100 --y 100 --json
ORCINUS computer click --app <app> --x 100 --y 100 --modifiers CmdOrCtrl+Shift --json
ORCINUS computer click --app <app> --element-index <index> --mouse-button right --json
ORCINUS computer click --app <app> --element-index <index> --mouse-button middle --json
ORCINUS computer perform-secondary-action --app <app> --element-index <index> --action <name> --json
ORCINUS computer set-value --app <app> --element-index <index> --value "text" --json
ORCINUS computer type-text --app <app> --text "text" --json
ORCINUS computer press-key --app <app> --key Return --json
ORCINUS computer hotkey --app <app> --key CmdOrCtrl+A --json
ORCINUS computer paste-text --app <app> --text "text" --json
ORCINUS computer scroll --app <app> (--element-index <index> | --x <x> --y <y>) --direction down --json
ORCINUS computer drag --app <app> --from-element-index <index> --to-element-index <index> --json
ORCINUS computer drag --app <app> --from-x 100 --from-y 100 --to-x 300 --to-y 300 --json
```

Use `--no-screenshot` only when pixels are not needed. Use `--text-stdin` or `--value-stdin` for sensitive text so payloads do not land in shell history; action payloads still pass through a short-lived local operation file on their way to the helper, so avoid sending secrets unless the user explicitly asked for them:

```bash
printf '%s' "$TEXT" | ORCINUS computer set-value --app <app> --element-index <index> --value-stdin --json
```

## Action Rules

- An action's verification is separate from whether its provider call succeeded:
  - `verified` means the changed value was read back.
  - `unverified (accessibility action unasserted)` means the accessibility call succeeded but no post-state assertion was made.
  - `unverified (synthetic input)` means input was fired into the void and is unverifiable.
  - Missing verification metadata is unverified, including responses from older runtimes.
  - Never report an unverified action as success. If it could have sent, submitted, bought, or deleted something, say the effect is unproven.
- Prefer semantic actions: `set-value` for editable fields, `click` for controls, and `perform-secondary-action` only for listed action names.
- After any UI-changing action, use the returned state or rerun `get-app-state` before choosing the next element index.
- Use `type-text` only after focusing a field and confirming the app has a focused text receiver; synthetic keyboard delivery is reported as unverified, so inspect the returned state before assuming text landed.
- Use `press-key` for single/navigation keys such as Return, Escape, Tab, and arrows. Use `hotkey` only for one modifier chord plus one key, such as `CmdOrCtrl+A` or `CmdOrCtrl+Shift+P`; prefer `CmdOrCtrl+...` for cross-platform combos.
- Use `click --modifiers <chord>` for modifier-clicks. Never synthesize separate modifier-down and modifier-up commands around a click; interruption can leave a modifier logically held.
- Some actions work in background apps, but this is app-dependent. If success does not change the UI, refresh state and choose a more semantic action or restore/focus the window.
- Coordinates are window-local; use coordinates from the latest screenshot/state for the same target window.

## Screenshots

`get-app-state` and actions request screenshots by default unless `--no-screenshot` is passed. A successful `--json` capture is normally saved at `result.screenshot.path` (under `$ORCINUS_COMPUTER_SCREENSHOT_TMPDIR` or `$TMPDIR/orcinus-computer-use/`, 24h retention); if that path is absent, use the inline base64 `result.screenshot.data`. Pretty output does not save images.

Use the tree for indexes/actions and the screenshot for visual confirmation; failed capture usually means hidden, minimized, off-screen, or permission-blocked.

Coordinates passed to `click`, `scroll`, and `drag` are window-local action coordinates. If the screenshot reports `scale` other than `1`, convert visual screenshot pixels before acting:

```text
action_x = screenshot_pixel_x / screenshot.scale
action_y = screenshot_pixel_y / screenshot.scale
```

Prefer element indexes or element frames from the tree when available. Use raw screenshot-derived coordinates only after checking the latest screenshot scale and window size.

## App Notes

Browsers: for Edge, Chrome, Safari, and similar browser windows, set the address/search field directly, then press Return. Do not assume raw typing went to the address bar. Use `--restore-window` when the browser is not already frontmost. Large tab strips may show only the active tab plus an "inactive browser tabs omitted" marker; treat that as intentional noise reduction and operate on the current page/address bar unless the user asked to manage tabs.

For browser-hosted forms such as Gmail compose, verify the focused UI element after each field action. Page text fields can expose accessibility actions without moving DOM focus; if a click or `set-value` does not change the focused receiver, use `Tab` / `Shift+Tab` from a known focused field or window-local coordinates from a fresh screenshot. Prefer `paste-text` into the verified focused field for draft bodies, then inspect the returned state before continuing.

```text
ORCINUS computer get-app-state --app com.microsoft.edgemac --restore-window --json
ORCINUS computer set-value --app com.microsoft.edgemac --element-index <addressBarIndex> --value "test123" --json
ORCINUS computer press-key --app com.microsoft.edgemac --key Return --json
```

Spotify: refresh after playback clicks; the UI often changes asynchronously.

Slack: the accessibility tree may be shallow while the screenshot contains useful information. Reading visible Slack UI is fine when requested; sending messages or triggering workflows still needs explicit permission.

## Errors

- `app_not_found`: run `list-apps` and retry with the bundle ID. If the target is a web app such as Gmail, choose the desktop browser app/window that contains it; do not retry `ORCINUS computer ... --app Gmail` unchanged because `orcinus computer` app selectors refer to desktop apps, not website names.
- `app_blocked`: stop; the target is intentionally blocked from computer-use.
- `window_not_found` / `window_stale`: run `list-windows`, choose a current selector, then rerun `get-app-state`.
- `window_not_focused`: retry once with `--restore-window`; if the message says restore was already requested, stop retrying restore and bring the app forward manually or check permissions. For editable fields prefer `set-value`, then inspect before assuming keyboard input worked.
- `element_not_found`: index is stale; run `get-app-state` again.
- `unsupported_capability`: the provider or desktop environment cannot do that action; use a semantic alternative or install the missing dependency if the message names one.
- `action_not_supported`: inspect the element's listed actions and retry with one of those names, or use click/set-value when appropriate.
- `value_not_settable`: the element cannot accept direct value writes; focus it and use keyboard input only when the returned state can be inspected.
- `element_not_clickable`: the element has no actionable frame; use a parent/child element with a frame or choose window-local coordinates from the latest screenshot.
- `invalid_argument`: fix the command flags; do not retry the same command unchanged.
- `action_timeout`: inspect current state before retrying, then use a simpler semantic action or `--no-screenshot` if observation is slow.
- `screenshot_failed`: use `--no-screenshot` if tree state is enough; if the message names Screen Recording or screenshots permission, run `ORCINUS computer permissions --id screenshots --json`.
- `accessibility_error`: run `ORCINUS computer capabilities --json`; if the message names Accessibility permission, run `ORCINUS computer permissions --id accessibility --json`.
- Empty tree or no screenshot: app may have no visible window, be minimized, or need permissions.
- Permission errors: run `ORCINUS computer permissions --json`, or `ORCINUS computer permissions --id accessibility --json` / `--id screenshots --json` when the message names one permission, use the setup UI, then retry.
