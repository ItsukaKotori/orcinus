import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAppStore } from '@/store'
import {
  getSettingsFocusedExecutionHostId,
  LOCAL_EXECUTION_HOST_ID,
  parseExecutionHostId,
  type ExecutionHostId
} from '../../../../shared/execution-host'
import { isEphemeralVmRuntimeEnvironment } from '../../../../shared/runtime-environments'
import type { AddRepoDialogStep } from './add-repo-dialog-types'
import { useSidebarHostScopeOptions } from './use-sidebar-host-scope-options'
import { canSelectAddRepoHost } from './add-repo-host-availability'
import { isWebClientLocation } from '@/lib/web-client-location'

export function useAddRepoHostSelection({
  isOpen,
  setStep
}: {
  isOpen: boolean
  setStep: (step: AddRepoDialogStep) => void
}): {
  hostOptions: ReturnType<typeof useSidebarHostScopeOptions>['hostOptions']
  selectedHostId: ExecutionHostId | null
  selectedParsedHost: ReturnType<typeof parseExecutionHostId>
  hostSelectorOpen: boolean
  setHostSelectorOpen: (open: boolean) => void
  handleSelectAddProjectHost: (hostId: ExecutionHostId) => Promise<void>
} {
  const settings = useAppStore((s) => s.settings)
  const runtimeEnvironments = useAppStore((s) => s.runtimeEnvironments)
  const { hostOptions } = useSidebarHostScopeOptions()
  const isWebClient = isWebClientLocation()
  const ephemeralRuntimeEnvironmentIds = useMemo(
    () =>
      new Set(
        runtimeEnvironments
          .filter(isEphemeralVmRuntimeEnvironment)
          .map((environment) => environment.id)
      ),
    [runtimeEnvironments]
  )
  const selectableHostOptions = useMemo(
    () =>
      hostOptions.filter((host) => {
        const parsed = parseExecutionHostId(host.id)
        return (
          parsed?.kind !== 'ssh' &&
          !(isWebClient && parsed?.kind === 'local') &&
          (parsed?.kind !== 'runtime' || !ephemeralRuntimeEnvironmentIds.has(parsed.environmentId))
        )
      }),
    [ephemeralRuntimeEnvironmentIds, hostOptions, isWebClient]
  )
  const [selectedAddProjectHostId, setSelectedAddProjectHostId] =
    useState<ExecutionHostId>(LOCAL_EXECUTION_HOST_ID)
  const [hostSelectorOpen, setHostSelectorOpen] = useState(false)
  const previousOpenRef = useRef(false)
  const pairedWebRuntimeHost = isWebClient
    ? selectableHostOptions.find((host) => host.kind === 'runtime' && canSelectAddRepoHost(host))
    : undefined

  const selectedHost =
    selectableHostOptions.find(
      (host) => host.id === selectedAddProjectHostId && canSelectAddRepoHost(host)
    ) ??
    pairedWebRuntimeHost ??
    selectableHostOptions.find(
      (host) => host.id === LOCAL_EXECUTION_HOST_ID && canSelectAddRepoHost(host)
    ) ??
    selectableHostOptions.find((host) => canSelectAddRepoHost(host))
  const selectedHostId = selectedHost?.id ?? (isWebClient ? null : LOCAL_EXECUTION_HOST_ID)
  const selectedParsedHost = parseExecutionHostId(selectedHostId)

  useEffect(() => {
    if (isOpen && !previousOpenRef.current) {
      const focusedHostId = getSettingsFocusedExecutionHostId(settings)
      const nextHostId = selectableHostOptions.some(
        (host) => host.id === focusedHostId && canSelectAddRepoHost(host)
      )
        ? focusedHostId
        : (pairedWebRuntimeHost?.id ?? (isWebClient ? null : LOCAL_EXECUTION_HOST_ID))
      if (nextHostId) {
        setSelectedAddProjectHostId(nextHostId)
      }
    }
    if (!isOpen) {
      setHostSelectorOpen(false)
    }
    previousOpenRef.current = isOpen
  }, [isOpen, isWebClient, pairedWebRuntimeHost?.id, selectableHostOptions, settings])

  const handleSelectAddProjectHost = useCallback(
    async (hostId: ExecutionHostId): Promise<void> => {
      const host = selectableHostOptions.find((candidate) => candidate.id === hostId)
      if (!host || !canSelectAddRepoHost(host)) {
        return
      }
      setSelectedAddProjectHostId(hostId)
      setStep('add')
    },
    [selectableHostOptions, setStep]
  )

  return {
    hostOptions: selectableHostOptions,
    selectedHostId,
    selectedParsedHost,
    hostSelectorOpen,
    setHostSelectorOpen,
    handleSelectAddProjectHost
  }
}
