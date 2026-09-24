import type { ProjectsApi } from '../../shared/preload-api/api/repository-api'
import type { Project } from '../../shared/project-types'
import { projectHostSetupProjectionFromRepos } from '../../shared/project-host-setup-projection'
import type { Repo } from '../../shared/repo-types'
import { withMethodFallback } from '../unimplemented-fallback'
import { invokeCommand } from './invoke'

/**
 * `projects` has no Rust command surface (spec §3.2/§10.4): the list views are
 * the shared TS projection of `repos_list`, so the identity-grouping logic
 * stays in one place. `update` only echoes `localWindowsRuntimePreference`
 * in memory — persisting it is the Windows follow-up.
 */
async function projectRepos(): Promise<ReturnType<typeof projectHostSetupProjectionFromRepos>> {
  const repos = await invokeCommand<Repo[]>('repos_list')
  return projectHostSetupProjectionFromRepos(repos)
}

export function createProjectsRealApi(): ProjectsApi {
  return withMethodFallback<ProjectsApi>('projects', {
    list: async () => [...(await projectRepos()).projects],
    listHostSetups: async () => [...(await projectRepos()).setups],
    update: async ({ projectId, updates }) => {
      const project = (await projectRepos()).projects.find((row) => row.id === projectId)
      if (!project) {
        return null
      }
      if (!('localWindowsRuntimePreference' in updates)) {
        return { ...project }
      }
      const next: Project = { ...project }
      if (updates.localWindowsRuntimePreference === undefined) {
        delete next.localWindowsRuntimePreference
      } else {
        next.localWindowsRuntimePreference = updates.localWindowsRuntimePreference
      }
      return next
    }
  })
}
