import { createAdeApi, type AdeApiOptions } from './create-api'

export function installAdeBridge(options?: AdeApiOptions): void {
  window.api = createAdeApi(options)
}
