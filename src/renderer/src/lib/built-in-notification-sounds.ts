// Why: orca 的 9 个内置提示音随仓库分发；renderer 经 Vite `?url` 直接拿到
// 构建产物 URL 播放（与 AppIconSelector 的资源导入同机制），无需宿主 IPC。
import twoToneUrl from '../../../../resources/notification-sounds/two-tone.mp3?url'
import bongUrl from '../../../../resources/notification-sounds/bong.mp3?url'
import thumpUrl from '../../../../resources/notification-sounds/thump.mp3?url'
import blipUrl from '../../../../resources/notification-sounds/blip.mp3?url'
import sonarUrl from '../../../../resources/notification-sounds/sonar.mp3?url'
import blopUrl from '../../../../resources/notification-sounds/blop.mp3?url'
import dingUrl from '../../../../resources/notification-sounds/ding.mp3?url'
import clackUrl from '../../../../resources/notification-sounds/clack.mp3?url'
import beepUrl from '../../../../resources/notification-sounds/beep.mp3?url'

export const BUILT_IN_NOTIFICATION_SOUND_IDS = [
  'two-tone',
  'bong',
  'thump',
  'blip',
  'sonar',
  'blop',
  'ding',
  'clack',
  'beep'
] as const

export type BuiltInNotificationSoundId = (typeof BUILT_IN_NOTIFICATION_SOUND_IDS)[number]

export const BUILT_IN_SOUND_URLS: Record<BuiltInNotificationSoundId, string> = {
  'two-tone': twoToneUrl,
  bong: bongUrl,
  thump: thumpUrl,
  blip: blipUrl,
  sonar: sonarUrl,
  blop: blopUrl,
  ding: dingUrl,
  clack: clackUrl,
  beep: beepUrl
}

export function builtInSoundUrl(id: string): string | null {
  return Object.hasOwn(BUILT_IN_SOUND_URLS, id)
    ? (BUILT_IN_SOUND_URLS as Record<string, string>)[id]
    : null
}
