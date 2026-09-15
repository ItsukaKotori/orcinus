export class UnimplementedBridgeError extends Error {
  readonly path: string
  constructor(path: string) {
    super(`ade bridge method not implemented yet: ${path}`)
    this.name = 'UnimplementedBridgeError'
    this.path = path
  }
}

function createRejectingMethod(prefix: string, method: string): () => Promise<never> {
  return async (..._args: unknown[]) => {
    console.warn(`[ade:bridge] unimplemented call ${prefix}.${method}`)
    throw new UnimplementedBridgeError(`${prefix}.${method}`)
  }
}

function createNamespace(prefix: string): Record<string, unknown> {
  const methods = new Map<string, (...args: unknown[]) => Promise<never>>()
  return new Proxy(
    {},
    {
      get(_target, property: string | symbol): unknown {
        // Why: a fabricated then would make await / Promise.resolve reject through
        // this fallback, and symbols are never method names.
        if (typeof property !== 'string' || property === 'then') return undefined
        if (!methods.has(property)) {
          methods.set(property, createRejectingMethod(prefix, property))
        }
        return methods.get(property)
      }
    }
  )
}

/** Same rejection contract as the namespace fallback, for a namespace that implements a few methods. */
export function withMethodFallback<T extends object>(prefix: string, partial: Partial<T>): T {
  const methods = new Map<string, (...args: unknown[]) => Promise<never>>()
  // SAFETY: implemented members pass through; every other string property fabricates a rejecting
  // async method, so a method-level partial still behaves as a full namespace at call sites.
  return new Proxy(partial as T, {
    get(target, property: string | symbol): unknown {
      // Why: a fabricated then would make await / Promise.resolve reject through
      // this fallback, and symbols are never method names.
      if (typeof property !== 'string') return undefined
      const existing = (target as Record<string, unknown>)[property]
      if (existing !== undefined) return existing
      if (property === 'then') return undefined
      if (!methods.has(property)) {
        methods.set(property, createRejectingMethod(prefix, property))
      }
      return methods.get(property)
    }
  })
}

export function withUnimplementedFallback<T extends object>(partial: Partial<T>): T {
  const namespaces = new Map<string, unknown>()
  // SAFETY: implemented members pass through untouched; the Proxy behind them fabricates every
  // missing namespace as a method fallback, so the partial behaves as a full T at call sites.
  return new Proxy(partial as T, {
    get(target, property: string): unknown {
      const existing = (target as Record<string, unknown>)[property]
      if (existing !== undefined) return existing
      if (!namespaces.has(property)) namespaces.set(property, createNamespace(property))
      return namespaces.get(property)
    }
  })
}
