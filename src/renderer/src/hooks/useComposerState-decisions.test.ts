import { describe, expect, it } from 'vitest'
import {
  getMatchingLinkedTaskSourceContext,
  resolveInitialWorkspaceRunSeed,
  resolveSmartGitHubCreateNames
} from './useComposerState'

describe('useComposerState public decisions', () => {
  it('preserves a user name for PR start points only when the fallback is non-empty', () => {
    expect(
      resolveSmartGitHubCreateNames({
        resolutionKind: 'pr-start-point',
        smartWorkspaceName: 'smart-name',
        smartDisplayName: 'Smart name',
        fallbackWorkspaceName: 'user-name',
        nameIsAutoManaged: false
      })
    ).toEqual({ workspaceName: 'user-name', displayName: undefined })
    expect(
      resolveSmartGitHubCreateNames({
        resolutionKind: 'pr-start-point',
        smartWorkspaceName: 'smart-name',
        smartDisplayName: 'Smart name',
        fallbackWorkspaceName: '',
        nameIsAutoManaged: false
      })
    ).toEqual({ workspaceName: 'smart-name', displayName: 'Smart name' })
  })

  it('preserves host-qualified task context while draft host values take precedence', () => {
    expect(
      resolveInitialWorkspaceRunSeed({
        draftProjectId: 'draft-project',
        draftHostId: 'ssh:draft',
        draftProjectHostSetupId: 'draft-setup',
        initialTaskSourceContext: {
          projectId: 'task-project',
          hostId: 'ssh:task',
          projectHostSetupId: 'task-setup'
        }
      })
    ).toEqual({
      projectId: 'draft-project',
      hostId: 'ssh:draft',
      projectHostSetupId: 'draft-setup'
    })
  })

  it('drops a task source context whose provider identity no longer matches the linked item', () => {
    const linkedItem = {
      provider: 'github' as const,
      type: 'issue' as const,
      number: 42,
      title: 'Fix it',
      url: 'https://github.com/orca/orca/issues/42'
    }
    const context = {
      kind: 'task-source' as const,
      provider: 'jira' as const,
      projectId: 'project-1',
      hostId: 'local' as const,
      providerIdentity: null
    }
    expect(getMatchingLinkedTaskSourceContext(linkedItem, context)).toBeNull()
  })
})
