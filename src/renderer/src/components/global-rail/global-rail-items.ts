import type { LucideIcon } from 'lucide-react'
import { CircleHelp, Folder, Puzzle, Settings } from 'lucide-react'

export type GlobalRailViewItem =
  | { kind: 'project'; label: '项目'; icon: LucideIcon; view: 'terminal' }
  | { kind: 'plugin-center'; label: '插件中心'; icon: LucideIcon; view: 'plugin-center' }

export type GlobalRailBottomItem =
  | { kind: 'settings'; label: '设置'; icon: LucideIcon }
  | { kind: 'help'; label: '帮助'; icon: LucideIcon }

export type GlobalRailItem = GlobalRailViewItem | GlobalRailBottomItem

export const GLOBAL_RAIL_ITEMS: GlobalRailViewItem[] = [
  { kind: 'project', label: '项目', icon: Folder, view: 'terminal' },
  { kind: 'plugin-center', label: '插件中心', icon: Puzzle, view: 'plugin-center' }
]

export const GLOBAL_RAIL_BOTTOM_ITEMS: GlobalRailBottomItem[] = [
  { kind: 'settings', label: '设置', icon: Settings },
  { kind: 'help', label: '帮助', icon: CircleHelp }
]
