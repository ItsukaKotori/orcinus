import { useState } from 'react'
import { Github } from 'lucide-react'
import type { GlobalSettings } from '../../../../shared/global-settings-types'
import type { TaskProvider } from '../../../../shared/task-providers'
import {
  normalizeVisibleTaskProviders,
  resolveVisibleTaskProvider
} from '../../../../shared/task-providers'
import { Button } from '@/components/ui/button'
import { useAppStore } from '@/store'
import { SearchableSetting } from './SearchableSetting'
import { SettingsSubsectionHeader } from './SettingsFormControls'
import { CodeHostSetupSteps } from './TaskSourceSimpleSetup'
import { TaskSourceProviderCard } from './TaskSourceProviderCard'
import {
  getStalledVisibleTaskProviders,
  resolveStickyAutoExpandedTaskProvider
} from './task-source-setup-state'
import { getTasksPaneSearchKeywords } from './tasks-search'
import { useTaskSourceProviderReadiness } from './use-task-source-provider-readiness'
import { translate } from '@/i18n/i18n'

type TasksPaneProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
}

const TASKS_PANE_PROVIDERS = ['github'] as const
type TasksPaneProvider = (typeof TASKS_PANE_PROVIDERS)[number]

const PROVIDER_META: Record<
  TasksPaneProvider,
  {
    label: string
    description: string
    Icon: (props: { className?: string }) => React.JSX.Element
  }
> = {
  github: {
    get label() {
      return translate('auto.components.settings.TasksPane.e14063e727', 'GitHub')
    },
    get description() {
      return translate(
        'auto.components.settings.TasksPane.githubDescription',
        'Browse GitHub issues and start workspaces from them.'
      )
    },
    Icon: ({ className }) => <Github className={className} />
  }
}

export function TasksPane({ settings, updateSettings }: TasksPaneProps): React.JSX.Element {
  const visibleProviders = normalizeVisibleTaskProviders(settings.visibleTaskProviders)
  const openSettingsPage = useAppStore((s) => s.openSettingsPage)
  const openSettingsTarget = useAppStore((s) => s.openSettingsTarget)
  const refreshPreflightStatus = useAppStore((s) => s.refreshPreflightStatus)
  const readinessByProvider = useTaskSourceProviderReadiness(visibleProviders)

  // Warn only about started-then-stalled setup; untouched providers are the default.
  const stalledVisible = getStalledVisibleTaskProviders(TASKS_PANE_PROVIDERS, readinessByProvider)
  // Sticky across rechecks so expanded setup cards are not unmounted.
  // Claim during render (not an effect): a layout effect elsewhere can force a
  // sync re-render before passive effects flush and collapse the open card.
  // useState (not a ref write) keeps render pure for React Doctor.
  const [previousAutoExpanded, setPreviousAutoExpanded] = useState<TaskProvider | null>(null)
  const autoExpandedProvider = resolveStickyAutoExpandedTaskProvider({
    providers: TASKS_PANE_PROVIDERS,
    readinessByProvider,
    previousAutoExpanded
  })
  if (autoExpandedProvider !== null && previousAutoExpanded === null) {
    setPreviousAutoExpanded(autoExpandedProvider)
  }

  const toggleProvider = (provider: TaskProvider): void => {
    const isVisible = visibleProviders.includes(provider)
    if (isVisible && visibleProviders.length === 1) {
      return
    }

    const nextProviders = isVisible
      ? visibleProviders.filter((entry) => entry !== provider)
      : TASKS_PANE_PROVIDERS.filter(
          (entry) => entry === provider || visibleProviders.includes(entry)
        )

    updateSettings({
      visibleTaskProviders: nextProviders,
      defaultTaskSource: resolveVisibleTaskProvider(settings.defaultTaskSource, nextProviders)
    })
  }

  const openIntegrations = (sectionId?: string): void => {
    openSettingsPage()
    openSettingsTarget({
      pane: 'integrations',
      repoId: null,
      ...(sectionId ? { sectionId } : {})
    })
  }

  return (
    <div className="space-y-6">
      <section className="space-y-3">
        <SettingsSubsectionHeader
          title={translate(
            'auto.components.settings.TasksPane.setupTitle',
            'Task management setup'
          )}
          description={translate(
            'auto.components.settings.TasksPane.setupDescription',
            'Finish connect + visibility for each provider in one place. At least one provider must stay visible.'
          )}
        />

        {stalledVisible.length > 0 ? (
          <div className="rounded-xl border border-amber-500/25 bg-amber-500/5 px-3.5 py-3 text-xs text-muted-foreground">
            <p className="font-medium text-amber-800 dark:text-amber-200">
              {translate(
                'auto.components.settings.TasksPane.incompleteBannerTitle',
                'Some visible providers still need setup'
              )}
            </p>
            <p className="mt-1">
              {translate(
                'auto.components.settings.TasksPane.incompleteBannerBody',
                'Hide providers you do not use, or expand a card and finish its steps.'
              )}
            </p>
          </div>
        ) : null}

        <SearchableSetting
          title={translate('auto.components.settings.TasksPane.f71d8a9dd3', 'Task Providers')}
          description={translate(
            'auto.components.settings.TasksPane.providersDescription',
            'Each card walks through connection plus whether it appears in Tasks.'
          )}
          keywords={getTasksPaneSearchKeywords()}
          className="space-y-3 py-2"
        >
          {TASKS_PANE_PROVIDERS.map((provider) => {
            const meta = PROVIDER_META[provider]
            const readiness = readinessByProvider[provider]
            const Icon = meta.Icon
            const visible = readiness.visible
            const canHide = visibleProviders.length > 1

            return (
              <TaskSourceProviderCard
                key={provider}
                icon={<Icon className="size-4" />}
                name={meta.label}
                description={meta.description}
                readiness={readiness}
                visible={visible}
                canHide={canHide}
                defaultExpanded={autoExpandedProvider === provider}
                onToggleVisible={() => toggleProvider(provider)}
              >
                <CodeHostSetupSteps
                  providerLabel={meta.label}
                  connected={readiness.connected}
                  checking={readiness.checking}
                  unavailable={readiness.unavailable}
                  visible={visible}
                  canHide={canHide}
                  onToggleVisible={() => toggleProvider(provider)}
                  onOpenIntegrations={() => openIntegrations()}
                  onRetryConnection={() => void refreshPreflightStatus({ force: true })}
                />
              </TaskSourceProviderCard>
            )
          })}
        </SearchableSetting>

        <p className="text-xs text-muted-foreground">
          {translate(
            'auto.components.settings.TasksPane.integrationsHint',
            'Credentials for all providers also live under'
          )}{' '}
          <Button
            type="button"
            variant="link"
            size="sm"
            className="h-auto p-0 text-xs align-baseline"
            onClick={() => openIntegrations()}
          >
            {translate('auto.components.settings.TasksPane.integrationsLink', 'Integrations')}
          </Button>
          .
        </p>
      </section>
    </div>
  )
}
