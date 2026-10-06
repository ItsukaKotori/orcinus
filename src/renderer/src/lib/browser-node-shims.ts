// Why: the shared agent-hook normalization graph imports node builtins for the
// main-process/relay listeners. The renderer reuses its pure claude mapping, so
// every node specifier in that graph aliases here (vite.config.ts). Buffer must
// be real enough for module-load constants; fs/crypto throw because every call
// site either never fires for claude hooks or fails open inside try/catch.

export class BrowserBuffer extends Uint8Array {
  static alloc(size: number): BrowserBuffer {
    return new BrowserBuffer(size)
  }

  static allocUnsafe(size: number): BrowserBuffer {
    return new BrowserBuffer(size)
  }

  static concat(chunks: Uint8Array[]): BrowserBuffer {
    const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0)
    const out = new BrowserBuffer(total)
    let offset = 0
    for (const chunk of chunks) {
      out.set(chunk, offset)
      offset += chunk.length
    }
    return out
  }

  static byteLength(value: string): number {
    return new TextEncoder().encode(value).length
  }

  static from(value: string | ArrayLike<number> | Iterable<number>): BrowserBuffer {
    if (typeof value === 'string') {
      return new BrowserBuffer(new TextEncoder().encode(value).buffer)
    }
    return new BrowserBuffer(value as ArrayLike<number>)
  }

  toString(): string {
    return new TextDecoder().decode(this)
  }
}

export const Buffer = BrowserBuffer

function unavailable(name: string): (...args: unknown[]) => never {
  return () => {
    throw new Error(`${name} is not available in the renderer (agent-hook node shim)`)
  }
}

export const closeSync = unavailable('fs.closeSync')
export const openSync = unavailable('fs.openSync')
export const readSync = unavailable('fs.readSync')
export const statSync = unavailable('fs.statSync')
export const lstatSync = unavailable('fs.lstatSync')
export const readdirSync = unavailable('fs.readdirSync')
export const readFileSync = unavailable('fs.readFileSync')
export const writeFileSync = unavailable('fs.writeFileSync')
export const existsSync = unavailable('fs.existsSync')
export const lstat = unavailable('fs/promises.lstat')
export const opendir = unavailable('fs/promises.opendir')
export const createHash = unavailable('crypto.createHash')
export const createHmac = unavailable('crypto.createHmac')
export const randomBytes = unavailable('crypto.randomBytes')
export const randomUUID = (): string => crypto.randomUUID()
export const homedir = (): string => ''
export const tmpdir = (): string => '/tmp'

export function join(...parts: string[]): string {
  return parts.filter(Boolean).join('/').replace(/\/{2,}/g, '/')
}

export function basename(value: string): string {
  return value.split('/').pop() ?? value
}

export function dirname(value: string): string {
  const parts = value.split('/')
  parts.pop()
  return parts.join('/') || '.'
}

export function extname(value: string): string {
  const base = basename(value)
  const index = base.lastIndexOf('.')
  return index > 0 ? base.slice(index) : ''
}

export function isAbsolute(value: string): boolean {
  return value.startsWith('/')
}

export const sep = '/'

export function resolve(...parts: string[]): string {
  const stack: string[] = []
  for (const segment of join(...parts).split('/')) {
    if (!segment || segment === '.') continue
    if (segment === '..') stack.pop()
    else stack.push(segment)
  }
  return `/${stack.join('/')}`
}

export function relative(from: string, to: string): string {
  const fromParts = resolve(from).split('/').filter(Boolean)
  const toParts = resolve(to).split('/').filter(Boolean)
  let common = 0
  while (common < fromParts.length && fromParts[common] === toParts[common]) common += 1
  const up = fromParts.slice(common).map(() => '..')
  return [...up, ...toParts.slice(common)].join('/')
}

export function readFile(): Promise<never> {
  return Promise.reject(new Error('fs/promises.readFile is not available in the renderer'))
}
