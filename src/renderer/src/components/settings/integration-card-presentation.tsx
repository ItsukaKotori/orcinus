import { cn } from '@/lib/utils'

export function useIntegrationCardShellClass(className?: string): string {
  return cn('rounded-xl border border-border bg-card px-4 py-3.5 shadow-xs', className)
}

export function useIntegrationSubordinateRowClass(className?: string): string {
  return cn('rounded-md border border-border/50 bg-muted/50 px-3 py-2', className)
}

export function useIntegrationCommandRowClass(): string {
  return cn(
    'flex items-center gap-2 font-mono text-xs',
    'rounded-md border border-border/50 bg-muted/50 px-3 py-2'
  )
}
