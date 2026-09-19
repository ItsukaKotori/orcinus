import type { StaticTopLevelView, TopLevelView } from './ui-chrome-types'

// Record keys are exhaustive so adding a top-level view also updates every
// persistence boundary that validates values loaded from disk or IPC.
const TOP_LEVEL_VIEW_LOOKUP: Record<StaticTopLevelView, true> = {
  terminal: true,
  settings: true,
  tasks: true,
  automations: true,
  space: true,
  artifacts: true,
  'plugin-center': true
}

// Plugin-hosted views are open-ended (`plugin:<pluginKey>`), so their keys can't
// live in the exhaustive Record above; the key charset is fixed so corrupt
// sidecar values can't smuggle arbitrary strings into the view type.
const PLUGIN_VIEW_RE = /^plugin:[a-z0-9][a-z0-9.-]*$/

export function isTopLevelView(value: unknown): value is TopLevelView {
  // Why: hasOwn (not `in`) so inherited keys like "constructor"/"__proto__" from a
  // corrupt sidecar can't pass as a view and leave the main surface blank.
  if (typeof value === 'string' && PLUGIN_VIEW_RE.test(value)) {
    return true
  }
  return typeof value === 'string' && Object.hasOwn(TOP_LEVEL_VIEW_LOOKUP, value)
}
