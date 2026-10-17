import type {
  BitbucketConnectArgs,
  BitbucketConnectionStatus
} from '../../bitbucket-credentials'

export type BitbucketApi = {
  connect: (
    args: BitbucketConnectArgs
  ) => Promise<{ ok: true; account: string | null } | { ok: false; error: string }>
  disconnect: () => Promise<void>
  status: () => Promise<BitbucketConnectionStatus>
}
