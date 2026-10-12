import type { PluginHostInstallSource, PluginScope } from '../../../../preload/api-types'
import { isAllowedPluginGitUrl } from '../../../../shared/plugins/plugin-install-lockfile'

export type PluginInstallScopeSelection = {
  scope: PluginScope
  repoId?: string
}

export type PluginInstallSourceParseResult =
  | { ok: true; source: PluginHostInstallSource }
  | {
      ok: false
      reason:
        | 'missing-local-path'
        | 'missing-git-url'
        | 'missing-git-ref'
        | 'invalid-git-url'
        | 'missing-repo-id'
    }

type ScopedFields = { scope: 'global' } | { scope: 'project'; repoId: string }

/** Project scope is meaningless without the repo it installs into, so an
 *  absent repoId is a validation failure rather than a silent global install. */
function resolveScopedFields(selection: PluginInstallScopeSelection): ScopedFields | null {
  if (selection.scope === 'global') {
    return { scope: 'global' }
  }
  const repoId = selection.repoId?.trim()
  return repoId ? { scope: 'project', repoId } : null
}

export function parsePluginInstallSource(
  kind: 'local-path' | 'git',
  input: string,
  selection: PluginInstallScopeSelection = { scope: 'global' }
): PluginInstallSourceParseResult {
  const scoped = resolveScopedFields(selection)
  if (!scoped) {
    return { ok: false, reason: 'missing-repo-id' }
  }
  const value = input.trim()
  if (kind === 'local-path') {
    return value
      ? { ok: true, source: { kind: 'local-path', path: value, ...scoped } }
      : { ok: false, reason: 'missing-local-path' }
  }
  if (!value) {
    return { ok: false, reason: 'missing-git-url' }
  }
  const separatorIndex = value.lastIndexOf('#')
  if (separatorIndex <= 0 || separatorIndex === value.length - 1) {
    return { ok: false, reason: 'missing-git-ref' }
  }
  const url = value.slice(0, separatorIndex).trim()
  const ref = value.slice(separatorIndex + 1).trim()
  if (!url) {
    return { ok: false, reason: 'missing-git-url' }
  }
  if (!isAllowedPluginGitUrl(url)) {
    return { ok: false, reason: 'invalid-git-url' }
  }
  if (!ref) {
    return { ok: false, reason: 'missing-git-ref' }
  }
  return { ok: true, source: { kind: 'git', url, ref, ...scoped } }
}
