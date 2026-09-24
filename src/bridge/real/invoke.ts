import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

/**
 * Renderer-side error contract (spec §6): Tauri rejects with the serialized
 * `BridgeError` shape `{ message }`, but renderer callers branch on `Error`
 * instances, so the raw payload is converted here. Anything already an `Error`
 * (including `UnimplementedBridgeError`) passes through untouched.
 */
export function toRendererError(error: unknown): Error {
  if (error instanceof Error) {
    return error
  }
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string') {
      return new Error(message)
    }
  }
  return new Error(String(error))
}

/**
 * Invoke one bridge command. Commands whose contract method has args take the
 * single `args` parameter (`invoke('repos_add', { args })`); no-arg commands
 * are called bare (`invoke('repos_list')`).
 */
export async function invokeCommand<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  try {
    return args === undefined ? await invoke<T>(command) : await invoke<T>(command, args)
  } catch (error) {
    throw toRendererError(error)
  }
}

/**
 * Subscribe to a Tauri event and return an idempotent unsubscriber. The
 * unsubscribe function is safe to call before `listen` resolves: the pending
 * registration is then dropped as soon as it arrives.
 */
export function subscribeToEvent<T>(event: string, callback: (payload: T) => void): () => void {
  let disposed = false
  let unlisten: (() => void) | null = null
  void listen<T>(event, (message) => callback(message.payload))
    .then((stop) => {
      if (disposed) {
        stop()
        return
      }
      unlisten = stop
    })
    .catch(() => {})
  return () => {
    disposed = true
    unlisten?.()
    unlisten = null
  }
}
