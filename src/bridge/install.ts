import { createAdeApi } from './create-api'

export function installAdeBridge(): void {
  window.api = createAdeApi()
}
