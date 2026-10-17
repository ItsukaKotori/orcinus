import type { HttpLinkSourceOwner } from '@/lib/http-link-routing'
import { openHttpLink } from '@/lib/http-link-routing'

export type HttpLinkAction = {
  external?: boolean
  label: string
  run: () => void | Promise<void>
}

/** Every http link opens in the system browser; the managed browser is gone. */
export function openRoutedHttpLink(
  url: string,
  deps: { sourceOwner?: HttpLinkSourceOwner } = {}
): void {
  openHttpLink(url, { sourceOwner: deps.sourceOwner })
}
