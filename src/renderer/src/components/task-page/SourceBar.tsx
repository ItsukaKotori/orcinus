import type { TaskPageComposerActionsModel } from '../use-task-page-composer-actions'
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip'
import { Button } from '@/components/ui/button'
import { translate } from '@/i18n/i18n'
import { X } from 'lucide-react'
import { toast } from 'sonner'
import { cn } from '@/lib/utils'
export function TaskPageSourceBar({
  model
}: {
  model: TaskPageComposerActionsModel
}): React.JSX.Element | null {
  const {
    openTaskPage,
    closeTaskPage,
    updateSettings,
    visibleSourceOptions,
    taskSource,
    taskSourceAvailabilityNoticeByProvider,
    taskSourceContextSummary,
    taskSourceManuallyChangedRef
  } = model
  return (
    <div className="flex items-center justify-between gap-2">
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {/* Why: Close is anchored left with the source icons for one compact band, clear of the app sidebar on the right. */}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-7 rounded-full"
              onClick={closeTaskPage}
              aria-label={translate('auto.components.TaskPage.1a06219d5c', 'Close tasks')}
            >
              <X className="size-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom" sideOffset={6}>
            {translate('auto.components.TaskPage.4826fd1ad8', 'Close · Esc')}
          </TooltipContent>
        </Tooltip>
        <div className="mx-1 h-5 w-px bg-border/50" aria-hidden />
        {visibleSourceOptions.map((source) => {
          const active = taskSource === source.id
          const sourceAvailabilityNotice = taskSourceAvailabilityNoticeByProvider[source.id] ?? null
          const sourceDisabled = source.disabled || sourceAvailabilityNotice?.blocking
          return (
            <Tooltip key={source.id}>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  disabled={sourceDisabled}
                  onClick={() => {
                    if (sourceAvailabilityNotice?.blocking) {
                      return
                    }
                    taskSourceManuallyChangedRef.current = true
                    openTaskPage(
                      {
                        taskSource: source.id
                      },
                      {
                        recordTasksInteraction: false
                      }
                    )
                    void updateSettings({
                      defaultTaskSource: source.id
                    }).catch(() => {
                      toast.error(
                        translate(
                          'auto.components.TaskPage.609532fae7',
                          'Failed to save default task source.'
                        )
                      )
                    })
                  }}
                  data-task-source={source.id}
                  aria-label={sourceAvailabilityNotice?.label ?? source.label}
                  aria-pressed={active}
                  className={cn(
                    'group flex h-8 w-8 items-center justify-center rounded-md border transition',
                    active
                      ? 'border-foreground/40 bg-muted/70 text-foreground shadow-sm'
                      : 'border-border/40 bg-transparent text-muted-foreground hover:bg-muted/40 hover:text-foreground',
                    sourceDisabled && 'cursor-not-allowed opacity-55'
                  )}
                >
                  <source.Icon className="size-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom" sideOffset={6}>
                {sourceAvailabilityNotice?.label ?? source.label}
              </TooltipContent>
            </Tooltip>
          )
        })}
        <div
          className="hidden min-w-0 max-w-[min(420px,40vw)] items-center rounded-md border border-border/50 bg-muted/35 px-2 py-1 text-xs text-muted-foreground sm:flex"
          title={taskSourceContextSummary.title}
        >
          <span className="truncate">{taskSourceContextSummary.label}</span>
        </div>
      </div>
    </div>
  )
}
