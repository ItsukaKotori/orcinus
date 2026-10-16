import { useAppStore } from '@/store'
import { getWindowsTerminalCapabilityOwnerKey } from '@/lib/windows-terminal-capabilities'

export function resolveWindowsTerminalCapabilityOwnerKey(args: {
  activeRuntimeEnvironmentId?: string | null
  sshConnectionId?: string | null
}): string {
  const activeEnvironmentId = args.activeRuntimeEnvironmentId?.trim() || null
  return getWindowsTerminalCapabilityOwnerKey(activeEnvironmentId, args.sshConnectionId)
}

export function useWindowsTerminalCapabilityOwnerKey(
  activeRuntimeEnvironmentId?: string | null,
  sshConnectionId?: string | null
): string {
  return useAppStore(() =>
    resolveWindowsTerminalCapabilityOwnerKey({ activeRuntimeEnvironmentId, sshConnectionId })
  )
}
