import type { ProviderAccountScope, ProviderRateLimitScope } from './provider-account-scope'
import { translate } from '@/i18n/i18n'

type ProviderHostScopeControlProps = {
  labelPrefix: string
  scope: ProviderAccountScope | ProviderRateLimitScope
  className?: string
}

export function ProviderHostScopeControl({
  labelPrefix,
  scope,
  className
}: ProviderHostScopeControlProps): React.JSX.Element {
  return (
    <div className={className}>
      {/* Why: integration cards can become narrow while Settings navigation
      remains visible, so the scope copy wraps instead of collapsing. */}
      <div className="min-w-[min(14rem,100%)] flex-1">
        <span className="font-medium text-foreground">
          {translate(
            'auto.components.settings.ProviderHostScopeControl.scope_label',
            '{{value0}}: {{value1}}',
            { value0: labelPrefix, value1: scope.label }
          )}
        </span>
        <div className="mt-0.5 text-muted-foreground">{scope.description}</div>
      </div>
    </div>
  )
}
