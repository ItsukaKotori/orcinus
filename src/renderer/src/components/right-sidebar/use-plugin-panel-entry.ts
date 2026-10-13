import { useEffect, useMemo, useRef, useState, type RefObject } from 'react'
import {
  createPanelInitMessage,
  PANEL_PING_TYPE,
  PLUGIN_PANEL_FRAME_NAME_PREFIX
} from '../../../../shared/plugins/plugin-panel-bridge'
import {
  PANEL_SHELL_COLOR_SCHEME_PLACEHOLDER,
  PANEL_SHELL_TOKENS_PLACEHOLDER
} from '../../../../shared/plugins/plugin-panel-shell'
import type { PluginScope } from '../../../../preload/api-types'
import {
  callPanelActionViaPreload,
  createPanelBridgeMessageHandler
} from './plugin-panel-bridge-host'
import { createPanelWatchdog } from './plugin-panel-watchdog'
import { buildPanelDesignTokenCss, currentPanelColorScheme } from './plugin-panel-design-token-css'
import { usePluginPanelThemeRevision } from './use-plugin-panel-theme-revision'

export type PluginPanelEntryState =
  | { status: 'loading' }
  | { status: 'error' }
  | { status: 'unresponsive' }
  | { status: 'ready'; shellHtml: string; documentRevision: number }

export type PluginPanelHealth = 'healthy' | 'error'

export type UsePluginPanelEntryOptions = {
  pluginKey: string | null
  panelId: string | null
  /** Main-area `contributes.tabs` entry id; set instead of `panelId` for
   *  `PluginTabContent` sessions. */
  tabId?: string | null
  /** `PluginTabBinding.instanceId`, forwarded to `readTabEntry`. */
  instanceId?: string
  /** Worktree the tab session binds to; forwarded to `readTabEntry`. */
  worktreeId?: string
  scope: PluginScope
  repoId?: string
  /** Stable identity for the iframe name and watchdog (tab key or `plugin:<key>`). */
  instanceKey: string
  onHealth?: (health: PluginPanelHealth) => void
  /** Persisted `workspace.openTab` params posted as `orca-panel-init` after the
   *  mounted document's first pong. Skipped when undefined. */
  initParams?: unknown
}

export type UsePluginPanelEntryResult = {
  entryState: PluginPanelEntryState
  iframeRef: RefObject<HTMLIFrameElement | null>
  panelDocument: string | null
  panelFrameName: string
  panelFrameKey: string | null
  loadedFrameKey: string | null
  handleFrameLoad: () => void
}

/** Fills the shell placeholders main cannot know (theme class + token
 *  values). First-occurrence replace: the prelude parses before any plugin
 *  content, so a plugin echoing the placeholder string is inert. */
function fillPanelShell(html: string): string {
  return html
    .replace(PANEL_SHELL_COLOR_SCHEME_PLACEHOLDER, currentPanelColorScheme())
    .replace(PANEL_SHELL_TOKENS_PLACEHOLDER, buildPanelDesignTokenCss())
}

/**
 * Shared host session for one plugin panel surface: reads the CSP-wrapped
 * shell + session token through preload, relays bridge calls, watches frame
 * liveness, and rebuilds the document when the entry or host theme changes.
 * Used by the right-sidebar `PluginPanel` and the global rail host view.
 */
export function usePluginPanelEntry(
  options: UsePluginPanelEntryOptions
): UsePluginPanelEntryResult {
  const { pluginKey, panelId, tabId, instanceId, worktreeId, scope, repoId, instanceKey } = options
  const [entryState, setEntryState] = useState<PluginPanelEntryState>({ status: 'loading' })
  const [sessionToken, setSessionToken] = useState<string | null>(null)
  const [loadedFrameKey, setLoadedFrameKey] = useState<string | null>(null)
  const iframeRef = useRef<HTMLIFrameElement | null>(null)
  const themeRevision = usePluginPanelThemeRevision()

  // Why: the watchdog outlives individual renders, so it reads the latest
  // callback through a ref instead of being rebuilt when the callback changes.
  const onHealthRef = useRef(options.onHealth)
  onHealthRef.current = options.onHealth
  // Why: the message handler is rebuilt only when the document/session changes;
  // refs keep it reading the latest init params and frame identity.
  const initParamsRef = useRef(options.initParams)
  initParamsRef.current = options.initParams
  const initPostedFrameKeyRef = useRef<string | null>(null)

  const panelShell = entryState.status === 'ready' ? entryState.shellHtml : null
  const panelDocument = panelShell ? fillPanelShell(panelShell) : null
  // Why: the shell bakes Orca's color scheme + design tokens into srcdoc, so the
  // frame must be rebuilt when the app theme changes, not only when the document does.
  const panelFrameKey =
    entryState.status === 'ready'
      ? `${instanceKey}:${entryState.documentRevision}:${themeRevision}`
      : null
  // Why: pongs can arrive outside render closures; a ref lets the stable
  // message handler know which document the init frame has already been sent to.
  const panelFrameKeyRef = useRef(panelFrameKey)
  panelFrameKeyRef.current = panelFrameKey
  const watchdog = useMemo(
    () =>
      createPanelWatchdog({
        sendPing: (pingId) =>
          iframeRef.current?.contentWindow?.postMessage({ type: PANEL_PING_TYPE, pingId }, '*'),
        onUnresponsive: () => {
          onHealthRef.current?.('error')
          setEntryState({ status: 'unresponsive' })
        }
      }),
    [instanceKey]
  )

  useEffect(() => {
    if (!sessionToken || !panelDocument) {
      return
    }
    let active = true
    const handler = createPanelBridgeMessageHandler({
      sessionToken,
      getPanelWindow: () => iframeRef.current?.contentWindow ?? null,
      callPanelAction: callPanelActionViaPreload,
      isActive: () => active,
      onPong: (pingId) => {
        watchdog.handlePong(pingId)
        // Spec §3.6: `orca-panel-init` follows the shell's first pong, proving
        // the guest listener is installed. Once per mounted document, not per
        // pong; the frame key changes when srcdoc/theme rebuilds the document.
        const frameKey = panelFrameKeyRef.current
        const params = initParamsRef.current
        if (!frameKey || params === undefined || initPostedFrameKeyRef.current === frameKey) {
          return
        }
        initPostedFrameKeyRef.current = frameKey
        iframeRef.current?.contentWindow?.postMessage(createPanelInitMessage(params), '*')
      }
    })
    window.addEventListener('message', handler)
    return () => {
      active = false
      window.removeEventListener('message', handler)
    }
  }, [panelDocument, sessionToken, watchdog])

  useEffect(() => {
    if (!panelFrameKey || loadedFrameKey !== panelFrameKey) {
      return
    }
    // The srcdoc prelude must install its pong listener before the first ping;
    // otherwise a healthy panel can lose the startup ping and be suspended.
    watchdog.start()
    return () => watchdog.stop()
  }, [loadedFrameKey, panelFrameKey, watchdog])

  useEffect(() => {
    if (!pluginKey || (!panelId && !tabId)) {
      return
    }
    let cancelled = false
    let currentHtml: string | null = null
    let documentRevision = 0
    setEntryState({ status: 'loading' })
    setSessionToken(null)
    const pluginsApi = window.api?.plugins
    if (!pluginsApi || (tabId && !pluginsApi.readTabEntry)) {
      onHealthRef.current?.('error')
      setEntryState({ status: 'error' })
      return
    }
    let loadGeneration = 0
    const load = (): void => {
      const generation = ++loadGeneration
      const entryRequest = tabId
        ? pluginsApi.readTabEntry!({
            pluginKey,
            tabId,
            instanceId: instanceId ?? instanceKey,
            scope,
            ...(repoId ? { repoId } : {}),
            worktreeId,
            ownerWindow: 'main'
          })
        : pluginsApi.readPanelEntry({ pluginKey, panelId: panelId!, scope, repoId })
      entryRequest
        .then((entry) => {
          if (cancelled || generation !== loadGeneration) {
            return
          }
          if (!entry) {
            currentHtml = null
            setSessionToken(null)
            onHealthRef.current?.('error')
            setEntryState({ status: 'error' })
            return
          }
          // Session rotation rebinds authority without replacing an unchanged
          // document or restarting its watchdog.
          setSessionToken(entry.sessionToken)
          onHealthRef.current?.('healthy')
          if (entry.html !== currentHtml) {
            currentHtml = entry.html
            documentRevision += 1
            setEntryState({
              status: 'ready',
              shellHtml: entry.html,
              documentRevision
            })
          }
        })
        .catch(() => {
          if (!cancelled && generation === loadGeneration) {
            currentHtml = null
            setSessionToken(null)
            onHealthRef.current?.('error')
            setEntryState({ status: 'error' })
          }
        })
    }
    load()
    const unsubscribe = pluginsApi.onChanged ? pluginsApi.onChanged(load) : null
    return () => {
      cancelled = true
      loadGeneration += 1
      unsubscribe?.()
    }
  }, [instanceId, panelId, pluginKey, repoId, scope, tabId, worktreeId])

  return {
    entryState,
    iframeRef,
    panelDocument,
    panelFrameName: `${PLUGIN_PANEL_FRAME_NAME_PREFIX}${instanceKey}`,
    panelFrameKey,
    loadedFrameKey,
    handleFrameLoad: () => setLoadedFrameKey(panelFrameKey)
  }
}
