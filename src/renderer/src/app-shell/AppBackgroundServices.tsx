import { AgentHibernationGate } from '../components/AgentHibernationGate'
import { AiVaultTabTitleSyncGate } from '../components/AiVaultTabTitleSyncGate'
import { PluginOpenTabGate } from '../components/PluginOpenTabGate'
import RetainedAgentsSyncGate from '../components/dashboard/RetainedAgentsSyncGate'
import { MacosTccPromptNoticeHost } from '../hooks/MacosTccPromptNoticeHost'

/**
 * App-level gates that render nothing. Each lives here rather than inside the surface that
 * needs it so its high-churn store subscriptions stay out of the App render tree.
 */
export function AppBackgroundServices(): React.JSX.Element {
  return (
    <>
      {/* Why: plugin language-pack discovery must not re-render the App shell. */}
      <MacosTccPromptNoticeHost />
      {/* Why: leaf-mounted retention sync keeps agent-status subscriptions out of the App render tree. */}
      <RetainedAgentsSyncGate />
      <AiVaultTabTitleSyncGate />
      <PluginOpenTabGate />
      <AgentHibernationGate />
    </>
  )
}
