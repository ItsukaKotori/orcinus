// Why first, and why the import-free shim: react-dom reads
// __REACT_DEVTOOLS_GLOBAL_HOOK__ once at module evaluation, so the global has to
// exist before it.
import './lib/react-devtools-commit-hook-shim'
import './assets/main.css'

import { StrictMode } from 'react'
import { useTranslation } from 'react-i18next'
import App from './App'
import { RecoverableRenderErrorBoundary } from './components/error-boundaries/RecoverableRenderErrorBoundary'
import { installAutomationHostDiagnostic } from './components/automations/automation-host-diagnostics'
import { applyDocumentTheme } from './lib/document-theme'
import { installTypingLatencyDiagnostic } from './lib/typing-latency/diagnostic'
import { shouldEnableReactGrab } from './lib/react-grab-dev-gate'
import { I18nProvider } from './i18n/I18nProvider'
import { translate } from './i18n/i18n'
import { getOrCreateRendererRoot } from './lib/react-renderer-root'
import { primeTerminalWebglAddon } from './lib/pane-manager/pane-webgl-renderer'
import { installBrowserClientPageRenderer } from './components/browser-pane/browser-client-page-renderer-installation'
import { installAdeBridge } from '../../bridge/install'

// Why first: boot-path hooks read window.api as soon as effects run, and there is no Electron
// preload in this shell, so the Phase 0 mock bridge must be in place before the first render.
installAdeBridge()

installTypingLatencyDiagnostic()
installAutomationHostDiagnostic()

if (
  import.meta.env.DEV &&
  shouldEnableReactGrab({
    dev: import.meta.env.DEV,
    enableFlag: import.meta.env.VITE_ENABLE_REACT_GRAB
  })
) {
  void import('react-grab').then(({ init }) => init())
  void import('react-grab/styles.css')
}

applyDocumentTheme('system', { disableTransitions: false })
const browserClientPageRenderer = installBrowserClientPageRenderer()
import.meta.hot?.dispose(() => browserClientPageRenderer?.dispose())

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('Renderer root element not found.')
}

function RendererRoot(): React.JSX.Element {
  useTranslation()
  return (
    <RecoverableRenderErrorBoundary
      boundaryId="app.root"
      title={translate('app.recoverableError.rootTitle', 'Orca hit a renderer error.')}
      description={translate(
        'app.recoverableError.rootDescription',
        'The app shell could not finish rendering. Retry to remount it, or relaunch Orca if the error persists.'
      )}
    >
      <App />
    </RecoverableRenderErrorBoundary>
  )
}

getOrCreateRendererRoot(rootElement, import.meta.hot?.data).render(
  <StrictMode>
    <I18nProvider>
      <RendererRoot />
    </I18nProvider>
  </StrictMode>
)

// Why here: the xterm WebGL addon is 243 KB, is only ever constructed once a
// terminal attaches (many frames away), and is needed by nothing during boot.
// Starting the load after the first render keeps it off the boot graph while
// leaving it resolved long before any pane can attach.
void primeTerminalWebglAddon()
