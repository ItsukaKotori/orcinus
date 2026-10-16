import { Button } from '@/components/ui/button'
import { TaskSourceShowInTasksStep } from './TaskSourceShowInTasksStep'
import { TaskSourceStepRow } from './TaskSourceStepRow'
import { translate } from '@/i18n/i18n'

type ConnectStepProps = {
  visible: boolean
  canHide: boolean
  onToggleVisible: () => void
  connected: boolean
  checking: boolean
}

function getConnectStepState(props: ConnectStepProps): 'in-progress' | 'done' | 'pending' {
  if (props.checking) {
    return 'in-progress'
  }
  return props.connected ? 'done' : 'pending'
}

export function CodeHostSetupSteps(
  props: ConnectStepProps & {
    providerLabel: string
    unavailable?: boolean
    onOpenIntegrations: () => void
    onRetryConnection: () => void
  }
): React.JSX.Element {
  const connectionDescription = props.unavailable
    ? translate(
        'auto.components.settings.TasksPane.connectionCheckUnavailable',
        "Orca couldn't check this connection. Try again, or open Integrations for setup details."
      )
    : translate(
        'auto.components.settings.TasksPane.connectCodeHostDescription',
        'Install and authenticate the CLI under Integrations so Orca can load issues.'
      )

  return (
    <ol className="divide-y divide-border/50">
      <TaskSourceStepRow
        index={1}
        state={getConnectStepState(props)}
        title={translate(
          'auto.components.settings.TasksPane.connectProviderTitle',
          'Connect {{provider}}',
          { provider: props.providerLabel }
        )}
        description={connectionDescription}
        action={
          <Button
            type="button"
            size="sm"
            variant={props.connected ? 'outline' : 'default'}
            onClick={props.unavailable ? props.onRetryConnection : props.onOpenIntegrations}
          >
            {props.unavailable
              ? translate('auto.components.settings.TasksPane.retryConnection', 'Try again')
              : props.connected
                ? translate('auto.components.settings.TasksPane.openIntegrations', 'Integrations')
                : translate(
                    'auto.components.settings.TasksPane.connectInIntegrations',
                    'Set up in Integrations'
                  )}
          </Button>
        }
      />
      <TaskSourceShowInTasksStep
        index={2}
        providerLabel={props.providerLabel}
        visible={props.visible}
        canHide={props.canHide}
        onToggleVisible={props.onToggleVisible}
      />
    </ol>
  )
}
