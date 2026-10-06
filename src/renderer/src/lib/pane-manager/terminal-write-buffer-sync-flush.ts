/**
 * Forces xterm's pending write queue to parse synchronously.
 *
 * Why this exists: after a webview reload the reloaded WKWebView page can leave
 * xterm's setTimeout-driven WriteBuffer loop starved for tens of seconds (IPC
 * events and React lifecycle run, but timer callbacks don't), so replayed
 * scrollback sits unparsed at depth>0/offset=0 and nothing paints until user
 * input takes xterm's synchronous `_didUserInput` fast path. Restore replay is
 * a one-shot bounded write, so draining it synchronously (the same primitive
 * CoreTerminal.resize uses before reflowing) removes the dependency on timer
 * resumption. All access is behind typeof guards: a vendored upgrade that
 * renames the internals degrades to a no-op and callers keep their async path.
 */

type FlushableWriteBuffer = {
  flushSync?: () => void
}

type FlushableTerminal = {
  write?: (data: string, callback?: () => void) => void
  _core?: {
    _writeBuffer?: FlushableWriteBuffer
  }
}

/** Returns true when a sync flush actually ran (callers may skip async waits). */
export function flushTerminalWriteBufferSync(terminal: unknown): boolean {
  try {
    const writeBuffer = (terminal as FlushableTerminal)._core?._writeBuffer
    if (typeof writeBuffer?.flushSync !== 'function') {
      return false
    }
    writeBuffer.flushSync()
    return true
  } catch {
    // A disposed terminal can throw mid-flush; the async path still resolves.
    return false
  }
}

/** True when xterm's write queue holds nothing left to parse (best-effort
 *  internal read; false when internals are unavailable). After a successful
 *  {@link flushTerminalWriteBufferSync} this is the signal that every queued
 *  byte parsed and all write callbacks fired — without waiting on a timer. */
export function isTerminalWriteBufferEmpty(terminal: unknown): boolean {
  try {
    const writeBuffer = (terminal as FlushableTerminal)._core?._writeBuffer
    if (!writeBuffer) {
      return false
    }
    const state = writeBuffer as unknown as Record<string, unknown>
    const depth = state._writeBuffer
    if (Array.isArray(depth)) {
      return depth.length === 0
    }
    return false
  } catch {
    return false
  }
}
