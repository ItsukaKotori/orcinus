import { z } from 'zod'

/**
 * Plugin capability model v0. The manifest declares capabilities, the user
 * consents against a fingerprint covering capabilities and worker trust, and the
 * host enforces at every plugin-callable boundary (panel bridge + worker host
 * API). Electron-free: shared by desktop main, headless serve, the relay
 * conformance path, and tests.
 *
 * v0 is a closed set of unscoped kinds so a typo (or a capability from a newer
 * Orca) fails manifest validation instead of silently granting nothing. The
 * fork adds `workspace:open-tab` (8 total); scoped kinds (net:fetch hosts,
 * process:exec globs) arrive in later phases.
 */

export const PLUGIN_CAPABILITY_KINDS = [
  'events:subscribe',
  'notifications:show',
  'secrets',
  'session:push',
  'settings:own',
  'storage',
  'terminal:send',
  'worker:call',
  'workspace:open-tab',
  'workspace:read'
] as const

export type PluginCapabilityKind = (typeof PLUGIN_CAPABILITY_KINDS)[number]

// Strict object (not a bare enum) so scoped fields can be added per-kind later
// without changing the manifest shape.
export const pluginCapabilitySchema = z.object({ kind: z.enum(PLUGIN_CAPABILITY_KINDS) }).strict()

export type PluginCapability = z.infer<typeof pluginCapabilitySchema>

/** Plain-language consent copy per capability. Shown verbatim in the install
 *  preview / consent dialog; keep each line honest about what is enforced. */
export const PLUGIN_CAPABILITY_DESCRIPTIONS: Record<PluginCapabilityKind, string> = {
  'events:subscribe':
    'Get notified when worktrees are created or removed and when agent status changes',
  'notifications:show': 'Show desktop notifications labeled with the plugin name',
  secrets: "Store and read secrets in the plugin's own encrypted vault",
  'session:push':
    "Allow this plugin's background worker to push live updates into its open panels and tabs.",
  'settings:own': "Read and change the plugin's own settings",
  storage: "Store data in the plugin's own storage folder",
  'terminal:send': 'Type text into a terminal you can see (always a specific terminal)',
  'worker:call':
    "Allow this plugin's panels and tabs to invoke its own background commands (worker RPC).",
  'workspace:open-tab': 'Open a new tab in the main workspace area',
  'workspace:read': 'Read the name, branch, and terminal list of your focused worktree'
}

/**
 * Canonical serialization of a capability set. Order- and duplicate-
 * insensitive so consent is stable across manifest reformatting;
 * key-sorted so future scoped fields cannot produce two encodings of the
 * same grant.
 */
export function canonicalizeCapabilitySet(capabilities: readonly PluginCapability[]): string {
  const encoded = capabilities.map((capability) =>
    JSON.stringify(
      Object.fromEntries(Object.entries(capability).sort(([a], [b]) => a.localeCompare(b)))
    )
  )
  return JSON.stringify([...new Set(encoded)].sort())
}

export function capabilityKinds(capabilities: readonly PluginCapability[]): PluginCapabilityKind[] {
  return [...new Set(capabilities.map((capability) => capability.kind))]
}
