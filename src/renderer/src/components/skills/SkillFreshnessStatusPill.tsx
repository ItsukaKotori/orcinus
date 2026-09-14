import { useSkillFreshness } from '@/hooks/useSkillFreshness'
import { translate } from '@/i18n/i18n'
import { IntegrationStatusPill } from '@/components/integration-status-pill'
import {
  getSkillFreshnessDisplayStatus,
  type SkillFreshnessDisplayStatus
} from '@/lib/skill-freshness-display-status'

function statusPill(status: SkillFreshnessDisplayStatus): React.JSX.Element {
  if (status === 'update-available') {
    return (
      <IntegrationStatusPill tone="attention">
        {translate(
          'auto.components.skills.SkillFreshnessStatusPill.updateAvailable',
          'Update available'
        )}
      </IntegrationStatusPill>
    )
  }
  if (status === 'needs-attention') {
    return (
      <IntegrationStatusPill tone="attention">
        {translate(
          'auto.components.skills.SkillFreshnessStatusPill.needsAttention',
          'Review skill'
        )}
      </IntegrationStatusPill>
    )
  }
  if (status === 'up-to-date') {
    return (
      <IntegrationStatusPill tone="connected">
        {translate('auto.components.skills.SkillFreshnessStatusPill.upToDate', 'Up to date')}
      </IntegrationStatusPill>
    )
  }
  return (
    <IntegrationStatusPill tone="connected">
      {translate('auto.components.skills.SkillFreshnessStatusPill.installed', 'Installed')}
    </IntegrationStatusPill>
  )
}

// Why: the setup rails' Installed pill is presence-only. Freshness knows more — that
// a safe update exists, that every copy is current, or that a copy is out of date
// somewhere the update cannot reach — and green must never stand in for that last
// case, which is real drift the user would otherwise have no way to see.
export function SkillFreshnessStatusPill({ skillName }: { skillName: string }): React.JSX.Element {
  const { inventory, loading, error } = useSkillFreshness()
  if (loading && !inventory) {
    return (
      <IntegrationStatusPill tone="neutral">
        {translate('auto.components.skills.SkillFreshnessStatusPill.checking', 'Checking...')}
      </IntegrationStatusPill>
    )
  }
  if (error && !inventory) {
    return (
      <IntegrationStatusPill tone="attention">
        {translate('auto.components.skills.SkillFreshnessStatusPill.checkFailed', 'Check failed')}
      </IntegrationStatusPill>
    )
  }
  const status = getSkillFreshnessDisplayStatus(inventory, skillName)
  // Why: Phase 0 removed the freshness review dialog, so the pill reports status only —
  // a Details action would open nothing.
  return <span className="inline-flex items-center gap-2">{statusPill(status)}</span>
}
