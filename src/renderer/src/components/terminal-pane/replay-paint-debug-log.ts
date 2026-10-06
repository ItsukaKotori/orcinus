// DEBUG(replay-paint): remove after diagnosis — console is unreadable during
// headed WKWebView automation, so breadcrumb lines are mirrored to an on-screen
// overlay that survives screenshots. Lines also ring-buffer in localStorage so
// TEARDOWN-time breadcrumbs (beforeunload) stay readable after the reload that
// destroys the page.

const STORAGE_KEY = 'orca.replay-paint-log'
const RING_LIMIT = 120

type ReplayPaintGlobal = {
  document?: Document
  localStorage?: { getItem: (k: string) => string | null; setItem: (k: string, v: string) => void }
}

function appendRing(message: string): void {
  const storage = (globalThis as ReplayPaintGlobal).localStorage
  if (!storage) {
    return
  }
  const prior = storage.getItem(STORAGE_KEY) ?? ''
  const lines = prior.length > 0 ? prior.split('\n') : []
  lines.push(message)
  while (lines.length > RING_LIMIT) {
    lines.shift()
  }
  storage.setItem(STORAGE_KEY, lines.join('\n'))
}

function replayPrevPageLines(target: HTMLElement): void {
  const storage = (globalThis as ReplayPaintGlobal).localStorage
  const prior = storage?.getItem(STORAGE_KEY) ?? ''
  if (prior.length === 0) {
    return
  }
  target.textContent += `${prior}\n── previous page above ──\n`
  storage?.setItem(STORAGE_KEY, '')
}

export function replayPaintDebugLog(message: string): void {
  console.debug(`[replay-paint] ${message}`)
  appendRing(message)
  try {
    const doc = (globalThis as ReplayPaintGlobal).document
    if (!doc?.body) {
      return
    }
    let box = doc.getElementById('replay-paint-log')
    if (!box) {
      box = doc.createElement('pre')
      box.id = 'replay-paint-log'
      box.style.cssText =
        'position:fixed;right:8px;top:8px;z-index:99999;background:#000c;color:#0f0;font:11px monospace;padding:6px;max-height:70vh;overflow:hidden;pointer-events:none;white-space:pre-wrap;max-width:46vw'
      doc.body.appendChild(box)
      replayPrevPageLines(box)
    }
    box.textContent += `${new Date().toISOString().slice(17, 23)} ${message}\n`
  } catch {
    // Overlay is best-effort.
  }
}
