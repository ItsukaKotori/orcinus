// Phase 0 test shim — remove with the electron type shim in Phase 1.
// Vitest aliases 'electron' here so a renderer test that reaches an Electron-backed
// src/main or preload module fails with one clear error instead of an unresolved-module
// crash or a silently faked Electron value. Only the members the reachable chains touch
// are exported; every property access, call, or construction throws.

const PHASE_0_ERROR = 'Electron is not available in this Tauri fork (Phase 0 electron test shim).'

function createThrowingMember(member: string): unknown {
  const fail = (access: string): never => {
    throw new Error(`${PHASE_0_ERROR} Accessed '${member}.${access}'.`)
  }
  const target = function electronStubMember(): never {
    return fail('()')
  }
  return new Proxy(target, {
    get: (_target, property) => {
      if (property === Symbol.toStringTag) {
        return `electron ${member} (Phase 0 stub)`
      }
      return fail(String(property))
    },
    apply: () => fail('()'),
    construct: () => fail('new')
  })
}

export const app = createThrowingMember('app')
export const contextBridge = createThrowingMember('contextBridge')
export const ipcRenderer = createThrowingMember('ipcRenderer')
export const webFrame = createThrowingMember('webFrame')
export const webUtils = createThrowingMember('webUtils')

export default createThrowingMember('electron')
