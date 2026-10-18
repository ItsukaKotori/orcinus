#!/usr/bin/env node
/**
 * Child-process entry for the out-of-process plugin worker (fork adaptation).
 *
 * Ported from `orca:src/main/plugins/plugin-host-entry.ts` +
 * `plugin-host-runtime.ts`: same frames and semantics, but the transport is
 * NDJSON over stdio instead of Node `fork` IPC. Frames are written to fd 1
 * with `writeSync` (a plain `process.exit()` can never truncate them) and
 * parent messages are read line-by-line from fd 0.
 *
 * Plain Node only: no npm dependencies, no Electron imports. Third-party
 * plugin code runs in this process; every parent frame is validated before
 * use and any escaped error is reported as `fatal` before exiting 1.
 */
import { createInterface } from 'node:readline'
import { writeSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { inspect } from 'node:util'

/** Closed capability set: the 7 oracle kinds + the fork's workspace:open-tab + the plugin UI↔worker channel kinds. */
const PLUGIN_CAPABILITY_KINDS = [
  'workspace:read',
  'terminal:send',
  'notifications:show',
  'storage',
  'secrets',
  'events:subscribe',
  'settings:own',
  'workspace:open-tab',
  'session:push',
  'worker:call'
]

/** Closed v0 event set, mirrored from `orca:src/shared/plugins/plugin-manifest.ts`. */
const PLUGIN_EVENT_NAMES = ['worktree.created', 'worktree.removed', 'agent.status.changed']

/** Frame text bound, mirrored from the oracle zod schemas (`.max(8192)`). */
const TEXT_MAX_BYTES = 8192

// Why: stdout is the NDJSON frame channel in this adaptation, so plugin
// console output goes to stderr (the host pipes it into worker logs) instead
// of corrupting frames.
function forwardConsoleToStderr(...args) {
  const line =
    args.map((value) => (typeof value === 'string' ? value : inspect(value))).join(' ') + '\n'
  writeSync(2, line)
}
console.log = forwardConsoleToStderr
console.info = forwardConsoleToStderr
console.debug = forwardConsoleToStderr
console.warn = forwardConsoleToStderr
console.error = forwardConsoleToStderr

function truncateUtf8(value, maxBytes = TEXT_MAX_BYTES) {
  const buffer = Buffer.from(value, 'utf8')
  if (buffer.length <= maxBytes) return value
  let end = maxBytes
  while (end > 0 && (buffer[end] & 0xc0) === 0x80) end -= 1
  return buffer.subarray(0, end).toString('utf8')
}

function toErrorMessage(error) {
  return error instanceof Error ? (error.stack ?? error.message) : String(error)
}

function send(frame) {
  writeSync(1, JSON.stringify(frame) + '\n')
}

function isNonEmptyString(value) {
  return typeof value === 'string' && value.length > 0
}

function isCallId(value) {
  return Number.isInteger(value) && value >= 0
}

/**
 * Hand-rolled equivalent of the oracle's zod discriminated union
 * (`pluginWorkerParentMessageSchema`). Returns the frame, or null to be
 * ignored. Zod strips unknown keys, so extra properties are tolerated too.
 */
function parseParentFrame(raw) {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null
  switch (raw.type) {
    case 'init':
      if (
        !isNonEmptyString(raw.pluginId) ||
        !isNonEmptyString(raw.pluginRoot) ||
        !isNonEmptyString(raw.mainEntry) ||
        !Array.isArray(raw.grantedCapabilities) ||
        !raw.grantedCapabilities.every((kind) => PLUGIN_CAPABILITY_KINDS.includes(kind))
      ) {
        return null
      }
      return raw
    case 'invokeCommand':
      if (!isCallId(raw.callId) || !isNonEmptyString(raw.commandId)) return null
      if (raw.session !== undefined) {
        if (
          typeof raw.session !== 'object' ||
          raw.session === null ||
          !isNonEmptyString(raw.session.token) ||
          raw.session.token.length > 128
        ) {
          return null
        }
      }
      return raw
    case 'deliverEvent':
      if (!isCallId(raw.eventId) || !PLUGIN_EVENT_NAMES.includes(raw.event)) return null
      if (!Object.prototype.hasOwnProperty.call(raw, 'payload')) return null
      return raw
    case 'hostResult':
      if (!isCallId(raw.callId) || typeof raw.ok !== 'boolean') return null
      if (raw.errorCode !== undefined && typeof raw.errorCode !== 'string') return null
      if (raw.error !== undefined && typeof raw.error !== 'string') return null
      return raw
    case 'shutdown':
      return raw
    default:
      return null
  }
}

const commandHandlers = new Map()
const eventHandlers = new Map()
const pendingHostCalls = new Map()
let nextHostCallId = 0
let initialized = false
let shuttingDown = false
let deactivate = null

function hostCall(method, params) {
  const callId = nextHostCallId++
  return new Promise((resolve, reject) => {
    pendingHostCalls.set(callId, { resolve, reject })
    send({ type: 'hostCall', callId, method, params })
  })
}

function makeCommandContext(message) {
  if (!message.session || typeof message.session.token !== 'string' || message.session.token.length === 0) {
    return undefined
  }
  const token = message.session.token
  return {
    session: {
      token,
      post(topic, payload) {
        return hostCall('session.post', { sessionToken: token, topic, payload })
      }
    }
  }
}

async function handleInit(message) {
  if (initialized) {
    send({ type: 'log', level: 'warn', message: 'ignoring duplicate init message' })
    return
  }
  initialized = true
  // Why: file URL import keeps ESM plugin entries working on Windows paths.
  // Manifest paths accept either separator, so split explicitly (oracle).
  const entryUrl = pathToFileURL(
    join(message.pluginRoot, ...message.mainEntry.split(/[\\/]/))
  ).href
  const module = await import(entryUrl)
  const activate = module.default
  if (typeof activate !== 'function') {
    throw new Error(`plugin entry ${message.mainEntry} has no default-exported activate function`)
  }
  if (module.deactivate !== undefined && typeof module.deactivate !== 'function') {
    throw new Error(`plugin entry ${message.mainEntry} has a non-function deactivate export`)
  }
  deactivate = module.deactivate ?? null
  const orca = {
    commands: {
      register(commandId, handler) {
        commandHandlers.set(commandId, handler)
      }
    },
    events: {
      on(event, handler) {
        const handlers = eventHandlers.get(event) ?? []
        handlers.push(handler)
        eventHandlers.set(event, handlers)
      }
    },
    host: {
      call(method, params) {
        return hostCall(method, params)
      }
    },
    grantedCapabilities: message.grantedCapabilities,
    log(message) {
      send({ type: 'log', level: 'info', message: truncateUtf8(String(message)) })
    }
  }
  await activate(orca)
  send({ type: 'ready', commands: [...commandHandlers.keys()] })
}

async function handleInvokeCommand(message) {
  const handler = commandHandlers.get(message.commandId)
  if (!handler) {
    send({
      type: 'commandResult',
      callId: message.callId,
      ok: false,
      error: `no handler registered for command ${message.commandId}`
    })
    return
  }
  try {
    const value = await handler(message.args, makeCommandContext(message))
    send({ type: 'commandResult', callId: message.callId, ok: true, value })
  } catch (error) {
    send({
      type: 'commandResult',
      callId: message.callId,
      ok: false,
      error: truncateUtf8(toErrorMessage(error))
    })
  }
}

async function handleDeliverEvent(message) {
  for (const handler of eventHandlers.get(message.event) ?? []) {
    try {
      await handler(message.payload)
    } catch (error) {
      send({ type: 'log', level: 'error', message: truncateUtf8(toErrorMessage(error)) })
    }
  }
  send({ type: 'eventAck', eventId: message.eventId })
}

function handleHostResult(message) {
  const pending = pendingHostCalls.get(message.callId)
  if (!pending) return
  pendingHostCalls.delete(message.callId)
  if (message.ok) {
    pending.resolve(message.value)
  } else {
    const error = new Error(message.error ?? 'host call failed')
    error.code = message.errorCode
    pending.reject(error)
  }
}

async function handleShutdown() {
  if (shuttingDown) return
  shuttingDown = true
  try {
    await deactivate?.()
  } catch (error) {
    send({ type: 'log', level: 'error', message: truncateUtf8(toErrorMessage(error)) })
  }
  process.exit(0)
}

async function handleMessage(raw) {
  const message = parseParentFrame(raw)
  if (!message) {
    send({ type: 'log', level: 'warn', message: 'ignoring malformed parent message' })
    return
  }
  try {
    switch (message.type) {
      case 'init':
        await handleInit(message)
        return
      case 'invokeCommand':
        await handleInvokeCommand(message)
        return
      case 'deliverEvent':
        await handleDeliverEvent(message)
        return
      case 'hostResult':
        handleHostResult(message)
        return
      case 'shutdown':
        await handleShutdown()
        return
    }
  } catch (error) {
    // Why: an init/activation failure leaves the worker useless; report and
    // die so the parent surfaces the error instead of hanging on ready.
    send({ type: 'fatal', error: truncateUtf8(toErrorMessage(error)) })
    process.exit(1)
  }
}

// Why: third-party plugin code runs here; an escaped rejection must not leave
// a zombie worker. Report the crash so the parent can supervise/restart.
function dieFatally(error) {
  try {
    send({ type: 'fatal', error: truncateUtf8(toErrorMessage(error)) })
  } catch {
    // Frame channel already gone; nothing left to report to.
  }
  process.exit(1)
}

process.on('uncaughtException', dieFatally)
process.on('unhandledRejection', dieFatally)

// Why: if the host dies without sending shutdown, stdin closes; exit instead
// of lingering as an orphaned Node process (stdio adaptation of 'disconnect').
process.stdin.on('end', () => process.exit(0))

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity })
lines.on('line', (line) => {
  if (line.trim() === '') return
  let parsed
  try {
    parsed = JSON.parse(line)
  } catch {
    send({ type: 'log', level: 'warn', message: 'ignoring malformed parent message' })
    return
  }
  void handleMessage(parsed)
})
