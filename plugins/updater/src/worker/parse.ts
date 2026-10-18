import type { OutdatedEntry, PackageKind } from '../shared/types'

type NpmOutdatedJson = Record<string, { current?: unknown; latest?: unknown }>

/** `npm outdated -g --json`: exit 1 with output when updates exist, `{}` when none. */
export function parseNpmOutdated(stdout: string): OutdatedEntry[] {
  const trimmed = stdout.trim()
  if (trimmed === '' || trimmed === '{}') {
    return []
  }
  const data = JSON.parse(trimmed) as NpmOutdatedJson
  const entries: OutdatedEntry[] = []
  for (const [name, value] of Object.entries(data)) {
    entries.push({
      manager: 'npm',
      kind: 'global',
      name,
      current: typeof value?.current === 'string' ? value.current : '',
      latest: typeof value?.latest === 'string' ? value.latest : ''
    })
  }
  return entries.sort((a, b) => a.name.localeCompare(b.name))
}

type BrewOutdatedV2Item = {
  name?: unknown
  installed_versions?: unknown
  current_version?: unknown
}

type BrewOutdatedV2 = {
  formulae?: BrewOutdatedV2Item[]
  casks?: BrewOutdatedV2Item[]
}

/** `brew outdated --json=v2`: exit 0 with `{ formulae, casks }`. */
export function parseBrewOutdated(stdout: string): OutdatedEntry[] {
  const trimmed = stdout.trim()
  if (trimmed === '') {
    return []
  }
  const data = JSON.parse(trimmed) as BrewOutdatedV2
  const entries: OutdatedEntry[] = []
  const collect = (kind: PackageKind, items: BrewOutdatedV2Item[] | undefined) => {
    for (const item of items ?? []) {
      if (typeof item?.name !== 'string' || item.name.length === 0) {
        continue
      }
      const current =
        Array.isArray(item.installed_versions) && typeof item.installed_versions[0] === 'string'
          ? (item.installed_versions[0] as string)
          : ''
      entries.push({
        manager: 'brew',
        kind,
        name: item.name,
        current,
        latest: typeof item.current_version === 'string' ? item.current_version : ''
      })
    }
  }
  collect('formula', data.formulae)
  collect('cask', data.casks)
  return entries.sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name))
}
