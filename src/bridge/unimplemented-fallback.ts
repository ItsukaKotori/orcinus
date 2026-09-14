export class UnimplementedBridgeError extends Error {
  readonly path: string
  constructor(path: string) {
    super(`ade bridge method not implemented yet: ${path}`)
    this.name = 'UnimplementedBridgeError'
    this.path = path
  }
}

function createNamespace(prefix: string): Record<string, unknown> {
  const methods = new Map<string, (...args: unknown[]) => Promise<never>>()
  return new Proxy(
    {},
    {
      get(_target, property: string): unknown {
        if (!methods.has(property)) {
          methods.set(property, async (..._args: unknown[]) => {
            console.warn(`[ade:bridge] unimplemented call ${prefix}.${property}`)
            throw new UnimplementedBridgeError(`${prefix}.${property}`)
          })
        }
        return methods.get(property)
      }
    }
  )
}

export function withUnimplementedFallback<T extends object>(partial: Partial<T>): T {
  const namespaces = new Map<string, unknown>()
  // SAFETY: implemented members pass through untouched; the Proxy behind them fabricates every
  // missing namespace as rejecting async methods, so the partial behaves as a full T at call sites.
  return new Proxy(partial as T, {
    get(target, property: string): unknown {
      const existing = (target as Record<string, unknown>)[property]
      if (existing !== undefined) return existing
      if (!namespaces.has(property)) namespaces.set(property, createNamespace(property))
      return namespaces.get(property)
    }
  })
}
