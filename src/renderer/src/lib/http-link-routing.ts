export type OpenHttpLinkOptions = {
  sourceOwner?: HttpLinkSourceOwner
}

export type HttpLinkSourceOwner =
  | { kind: 'local' }
  | { kind: 'runtime'; runtimeEnvironmentId: string }
  | { kind: 'ssh'; connectionId: string }
  | { kind: 'unknown' }

// Scope: http(s) URLs only. file: URIs and in-worktree markdown targets are
// owned by resolveMarkdownLinkTarget and must stay on that path — this helper
// is only invoked on target.kind === 'external' (and for the terminal's http
// branch).
export function openHttpLink(url: string, opts: OpenHttpLinkOptions = {}): void {
  const { sourceOwner } = opts
  if (sourceOwner?.kind === 'unknown') {
    return
  }
  // Why: the managed browser is gone; every http link opens in the system browser.
  void window.api.shell.openUrl(url)
}

export async function resolveLocalhostHttpLinkDisplayUrl(
  _url: string,
  _sourceOwner?: HttpLinkSourceOwner
): Promise<string | null> {
  // Why: localhost worktree labels were derived from the removed workspace port
  // scan, so no display URL can be resolved.
  return null
}
