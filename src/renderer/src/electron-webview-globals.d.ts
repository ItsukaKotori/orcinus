// The Phase 0 electron-type-shim is gone. The renderer is the only remaining
// typecheck program that touches the Electron namespace: browser-pane code types
// the host `<webview>` tag through the global `Electron.*` surface. The fork does
// not ship Electron, so this is the minimal declaration set still referenced.

declare namespace Electron {
  type WebviewTag = any
  type DidFailLoadEvent = any
  type DidRedirectNavigationEvent = any
  type DidStartNavigationEvent = any
  type FindInPageOptions = any
  type FoundInPageEvent = any
  type PageTitleUpdatedEvent = any
}
