import { z } from 'zod'

export const tabContentTypeSchema = z.enum([
  'terminal',
  'editor',
  'diff',
  'conflict-review',
  'check-details',
  'agent-session',
  'git-log'
])

export const workspaceVisibleTabTypeSchema = z.enum([
  'terminal',
  'editor',
  'agent-session',
  'git-log'
])
