import { Updater, type UpdaterDeps } from './jobs'
import { defaultAdapters } from './managers'

export type SessionContext = {
  token: string
  post: (topic: string, payload: unknown) => Promise<unknown>
}

export type CommandContext = { session?: SessionContext }

export type OrcaSdk = {
  commands: { register: (id: string, handler: (args: unknown, ctx?: CommandContext) => unknown) => void }
  host: { call: (method: string, params: unknown) => Promise<unknown> }
  log: (message: string) => void
  events: { on: (event: string, handler: (payload: unknown) => void) => void }
}

export function createUpdater(orca: OrcaSdk, overrides: Partial<UpdaterDeps> = {}): Updater {
  return new Updater({
    adapters: defaultAdapters(),
    storageGet: async (key) => {
      const result = (await orca.host.call('storage.get', { key })) as { value?: unknown }
      return result?.value ?? null
    },
    storageSet: async (key, value) => {
      await orca.host.call('storage.set', { key, value })
    },
    postTo: async (sessionToken, topic, payload) => {
      await orca.host.call('session.post', { sessionToken, topic, payload })
    },
    now: () => Date.now(),
    ...overrides
  })
}

export function registerCommands(orca: OrcaSdk, updater: Updater): void {
  const withSession =
    <T>(run: () => Promise<T>) =>
    (_args: unknown, ctx?: CommandContext): Promise<T> => {
      if (ctx?.session) {
        updater.rememberSession(ctx.session.token)
      }
      return run()
    }

  orca.commands.register('check', withSession(() => updater.check()))
  orca.commands.register('update-all', withSession(() => updater.updateAll()))
  orca.commands.register('cancel', withSession(() => updater.cancel()))
  orca.commands.register('snapshot', withSession(() => updater.snapshot()))
  orca.commands.register('set-managers', (args, ctx) => {
    if (ctx?.session) {
      updater.rememberSession(ctx.session.token)
    }
    const managers = (args as { managers?: unknown } | undefined)?.managers
    const ids = Array.isArray(managers)
      ? managers.filter((id): id is string => typeof id === 'string')
      : []
    return updater.setManagers(ids)
  })
  orca.commands.register('open', (_args, ctx) => {
    if (ctx?.session) {
      updater.rememberSession(ctx.session.token)
    }
    return orca.host.call('workspace.openTab', { tabId: 'updates' })
  })
}

export default function activate(orca: OrcaSdk): void {
  registerCommands(orca, createUpdater(orca))
  orca.log('updater activated')
}
