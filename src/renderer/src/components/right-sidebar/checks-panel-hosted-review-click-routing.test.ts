import { beforeEach, describe, expect, it, vi } from 'vitest'
import { openChecksPanelHostedReviewUrl } from './checks-panel-hosted-review-click-routing'

const { openHttpLinkMock } = vi.hoisted(() => ({ openHttpLinkMock: vi.fn() }))

vi.mock('@/lib/http-link-routing', () => ({
  openHttpLink: openHttpLinkMock
}))

beforeEach(() => {
  openHttpLinkMock.mockReset()
})

describe('checks panel hosted review click routing', () => {
  it('opens hosted review URLs in the system browser', () => {
    openChecksPanelHostedReviewUrl({ url: 'https://github.com/acme/widgets/pull/123' })

    expect(openHttpLinkMock).toHaveBeenCalledWith('https://github.com/acme/widgets/pull/123')
  })
})
