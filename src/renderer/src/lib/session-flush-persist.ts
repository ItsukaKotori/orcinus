/**
 * `session:flush-requested` handler body (spec §3.4), factored for tests:
 * capture buffers + resume records, then write the full payload directly —
 * bypassing the 150ms debounced subscriber, whose pending write would race
 * the ack.
 */
export type SessionFlushPersistDeps = {
  captureAll: () => void
  captureTranscripts: () => Promise<void>
  buildPayload: () => Record<string, unknown>
  canPersist: () => boolean
  patch: (payload: Record<string, unknown>) => Promise<void>
  flush: () => Promise<void>
}

export function createSessionFlushPersist(
  deps: SessionFlushPersistDeps
): () => Promise<void> {
  return async () => {
    deps.captureAll()
    await deps.captureTranscripts()
    if (!deps.canPersist()) {
      return
    }
    await deps.patch(deps.buildPayload())
    await deps.flush()
  }
}
