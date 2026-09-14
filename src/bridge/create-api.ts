import type { PreloadApi } from '../preload/api-types'
import { createAppApi } from './mock/app-api'
import { createCliApi } from './mock/cli-api'
import { createOnboardingApi } from './mock/onboarding-api'
import { createPluginsApi } from './mock/plugins-api'
import { createReposApi } from './mock/repos-api'
import { createRuntimeEnvironmentsApi } from './mock/runtime-environments-api'
import { createSettingsApi } from './mock/settings-api'
import { createUiApi } from './mock/ui-api'
import { createRemoteWorkspaceApi, createSessionApi } from './mock/workspace-session-api'
import { createWorktreesApi } from './mock/worktrees-api'
import { withUnimplementedFallback } from './unimplemented-fallback'

export function createAdeApi(): PreloadApi {
  // SAFETY: mock 域覆盖 Phase 0 启动与 UI 所需命名空间；其余经 Proxy 兜底为“未实现”拒绝，
  // 因此断言为 PreloadApi 在运行期仍保持“调用必为函数”的契约。
  const partial: Partial<PreloadApi> = {
    app: createAppApi(),
    cli: createCliApi(),
    onboarding: createOnboardingApi(),
    plugins: createPluginsApi(),
    repos: createReposApi(),
    remoteWorkspace: createRemoteWorkspaceApi(),
    runtimeEnvironments: createRuntimeEnvironmentsApi(),
    session: createSessionApi(),
    settings: createSettingsApi(),
    ui: createUiApi(),
    worktrees: createWorktreesApi()
  }
  return withUnimplementedFallback(partial)
}
