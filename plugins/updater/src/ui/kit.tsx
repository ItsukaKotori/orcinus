import type { ButtonHTMLAttributes, ReactNode } from 'react'

export function buttonClass(
  variant: 'default' | 'primary' | 'danger' = 'default',
  disabled = false
): string {
  const base =
    'inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-xs font-medium transition-colors'
  if (disabled) {
    return `${base} border-border bg-muted text-muted-foreground`
  }
  if (variant === 'primary') {
    return `${base} border-primary bg-primary text-primary-foreground hover:opacity-90`
  }
  if (variant === 'danger') {
    return `${base} border-destructive bg-transparent text-destructive hover:bg-destructive/10`
  }
  return `${base} border-border bg-background text-foreground hover:bg-accent`
}

export function Button({
  variant = 'default',
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: 'default' | 'primary' | 'danger' }) {
  return (
    <button type="button" className={`${buttonClass(variant, props.disabled === true)} ${className ?? ''}`} {...props} />
  )
}

export function Chip({
  selected = false,
  children,
  onClick
}: {
  selected?: boolean
  children: ReactNode
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        selected
          ? 'rounded-md bg-primary px-2 py-0.5 text-[11px] text-primary-foreground'
          : 'rounded-md bg-muted px-2 py-0.5 text-[11px] text-muted-foreground hover:text-foreground'
      }
    >
      {children}
    </button>
  )
}

export function Toggle({
  checked,
  onChange,
  label
}: {
  checked: boolean
  onChange: (next: boolean) => void
  label: string
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-[18px] w-[30px] shrink-0 rounded-full transition-colors ${checked ? 'bg-primary' : 'bg-border'}`}
    >
      <span
        className={`absolute top-[2px] h-[14px] w-[14px] rounded-full bg-background shadow transition-all ${checked ? 'left-[14px]' : 'left-[2px]'}`}
      />
    </button>
  )
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={`animate-pulse rounded-md bg-muted ${className ?? ''}`} />
}
