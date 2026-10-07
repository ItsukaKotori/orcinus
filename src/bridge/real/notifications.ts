import {
  isPermissionGranted,
  removeActive,
  requestPermission,
  sendNotification
} from '@tauri-apps/plugin-notification'
import { builtInSoundUrl } from '../../renderer/src/lib/built-in-notification-sounds'
import type { PreloadApi } from '../../shared/preload-api/api-types'
import type {
  NotificationDispatchRequest,
  NotificationPermissionStatusResult,
  NotificationSoundResult
} from '../../shared/notification-settings-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { getBootstrap } from './bootstrap'
import { invokeCommand } from './invoke'

const NOTIFICATION_COOLDOWN_MS = 5_000
const MAX_RECENT_NOTIFICATION_KEYS = 50
const recentDesktopNotifications = new Map<string, number>()
const playingSoundPaths = new Set<string>()

/** 规格 §3.8 裁决 1：标题/正文取最不惊讶的稳定字段（fork 文案的 2B 子集）。 */
export function buildNotificationCopy(args: NotificationDispatchRequest): {
  title: string
  body: string
} {
  const title = args.terminalTitle?.trim() || args.worktreeLabel?.trim() || 'Orcinus'
  const body =
    args.agentLastAssistantMessage?.trim() ||
    args.agentPrompt?.trim() ||
    (args.agentState === 'waiting'
      ? 'Waiting for input'
      : args.agentState === 'done'
        ? 'Task complete'
        : args.worktreeLabel?.trim() || 'Agent activity')
  return { title, body }
}

export function reserveNotificationCooldown(
  map: Map<string, number>,
  key: string,
  now: number
): boolean {
  const last = map.get(key)
  // Why `undefined` (not `?? 0`): an epoch-adjacent `now` must still count as the
  // first reservation; only a recorded prior send can start a cooldown window.
  if (last !== undefined && now - last < NOTIFICATION_COOLDOWN_MS) {
    return false
  }
  map.delete(key)
  map.set(key, now)
  while (map.size > MAX_RECENT_NOTIFICATION_KEYS) {
    const oldest = map.keys().next()
    if (oldest.done) {
      break
    }
    map.delete(oldest.value)
  }
  return true
}

/** 插件 id 是 32-bit int，稳定字符串 id 经 FNV-1a 映射（裁决 2）。 */
export function hashNotificationId(id: string): number {
  let hash = 0x811c9dc5
  for (let index = 0; index < id.length; index += 1) {
    hash ^= id.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193)
  }
  return hash | 0
}

function platformForPermissionStatus(): NodeJS.Platform {
  const bootstrap = getBootstrap()
  const platform = bootstrap?.platform?.platform
  return (platform as NodeJS.Platform | undefined) ?? 'darwin'
}

function playAudio(url: string, volume: number | null): Promise<void> {
  return new Promise((resolve, reject) => {
    const audio = new Audio(url)
    if (volume !== null) {
      // Why: renderer contract is 0..100 (preload divides by 100); Audio.volume is 0..1.
      audio.volume = Math.min(1, Math.max(0, volume))
    }
    audio.onended = () => resolve()
    audio.onerror = () => reject(new Error('playback failed'))
    void audio.play().catch(reject)
  })
}

export function createNotificationsRealApi(): PreloadApi['notifications'] {
  return withMethodFallback<PreloadApi['notifications']>('notifications', {
    getDesktopAwayState: async () => undefined,
    dispatch: async (args) => {
      const settings = getBootstrap()?.settings?.notifications
      if (settings && !settings.enabled) {
        return { delivered: false, reason: 'disabled' }
      }
      if (
        settings &&
        args.source === 'agent-task-complete' &&
        !settings.agentTaskComplete
      ) {
        return { delivered: false, reason: 'source-disabled' }
      }
      if (settings && args.source === 'terminal-bell' && !settings.terminalBell) {
        return { delivered: false, reason: 'source-disabled' }
      }
      if (
        args.source !== 'test' &&
        settings?.suppressWhenFocused &&
        args.isActiveWorktree &&
        typeof document !== 'undefined' &&
        document.hasFocus()
      ) {
        return { delivered: false, reason: 'suppressed-focus' }
      }
      if (args.source !== 'test') {
        const dedupeKey = args.worktreeId ?? args.worktreeLabel ?? 'global'
        if (!reserveNotificationCooldown(recentDesktopNotifications, dedupeKey, Date.now())) {
          return { delivered: false, reason: 'cooldown' }
        }
      }
      let granted = await isPermissionGranted()
      if (!granted) {
        granted = (await requestPermission()) === 'granted'
      }
      if (!granted) {
        return { delivered: false, reason: 'blocked-by-system' }
      }
      const copy = buildNotificationCopy(args)
      sendNotification({ title: copy.title, body: copy.body })
      return { delivered: true }
    },
    dismiss: async (ids) => {
      const unique = Array.from(new Set(ids.filter((id) => typeof id === 'string' && id.length > 0)))
      if (unique.length === 0) {
        return { dismissed: 0 }
      }
      try {
        await removeActive(unique.map((id) => ({ id: hashNotificationId(id) })))
        return { dismissed: unique.length }
      } catch {
        return { dismissed: 0 }
      }
    },
    openSystemSettings: () => invokeCommand('notifications_open_system_settings'),
    getPermissionStatus: async (): Promise<NotificationPermissionStatusResult> => ({
      supported: true,
      platform: platformForPermissionStatus(),
      requested: await isPermissionGranted()
    }),
    probeDelivery: async () => {
      if (await isPermissionGranted()) {
        return { state: 'delivered', authoritative: false }
      }
      const permission = await requestPermission()
      if (permission === 'granted') {
        return { state: 'delivered', authoritative: false }
      }
      if (permission === 'denied') {
        return { state: 'blocked', authoritative: false }
      }
      return { state: 'awaiting-decision', authoritative: false }
    },
    playSound: async (options): Promise<NotificationSoundResult> => {
      const settings = getBootstrap()?.settings?.notifications
      const volume = typeof options?.volume === 'number' ? options.volume / 100 : null
      const soundId = settings?.customSoundId
      if (soundId && soundId !== 'custom' && soundId !== 'system') {
        const url = builtInSoundUrl(soundId)
        if (!url) {
          return { played: false, reason: 'missing-path' }
        }
        if (options?.force !== true && playingSoundPaths.has(soundId)) {
          return { played: false, reason: 'deduped' }
        }
        playingSoundPaths.add(soundId)
        try {
          await playAudio(url, volume)
          return { played: true }
        } catch {
          return { played: false, reason: 'playback-failed' }
        } finally {
          playingSoundPaths.delete(soundId)
        }
      }
      const path = settings?.customSoundPath
      if (!path || soundId !== 'custom') {
        return { played: false, reason: 'missing-path' }
      }
      // Why: preload semantics — `force` replays while the same path is still ringing.
      if (options?.force !== true && playingSoundPaths.has(path)) {
        return { played: false, reason: 'deduped' }
      }
      const loaded = await invokeCommand<{
        ok: boolean
        dataBase64?: string
        mimeType?: string
        reason?: string
      }>('notifications_read_sound', { args: { path } })
      if (!loaded.ok || !loaded.dataBase64 || !loaded.mimeType) {
        const reason = loaded.reason
        return {
          played: false,
          reason:
            reason === 'missing-path' ||
            reason === 'invalid-path' ||
            reason === 'unsupported-type' ||
            reason === 'too-large' ||
            reason === 'read-failed'
              ? reason
              : 'read-failed'
        }
      }
      const bytes = Uint8Array.from(atob(loaded.dataBase64), (char) => char.charCodeAt(0))
      const objectUrl = URL.createObjectURL(new Blob([bytes], { type: loaded.mimeType }))
      playingSoundPaths.add(path)
      try {
        await playAudio(objectUrl, volume)
        return { played: true }
      } catch {
        return { played: false, reason: 'playback-failed' }
      } finally {
        playingSoundPaths.delete(path)
        URL.revokeObjectURL(objectUrl)
      }
    }
  })
}
