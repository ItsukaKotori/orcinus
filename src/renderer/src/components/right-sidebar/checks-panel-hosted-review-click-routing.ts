import { openHttpLink } from '@/lib/http-link-routing'

/** Opens a hosted review URL in the system browser. */
export function openChecksPanelHostedReviewUrl({ url }: { url: string }): void {
  openHttpLink(url)
}
