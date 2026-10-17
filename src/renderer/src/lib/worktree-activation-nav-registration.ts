import { setWorktreeNavActivator } from '@/store/slices/worktree-nav-history'

type ActivateFn = (worktreeId: string) => unknown

export function registerWorktreeActivation(activate: ActivateFn): void {
  setWorktreeNavActivator(activate)
}
