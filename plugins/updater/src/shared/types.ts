/** Shared domain types between the updater worker and its UI bundles. */

export type ManagerId = 'npm' | 'brew'

export type PackageKind = 'global' | 'formula' | 'cask'

export type OutdatedEntry = {
  manager: ManagerId
  kind: PackageKind
  name: string
  current: string
  latest: string
}
