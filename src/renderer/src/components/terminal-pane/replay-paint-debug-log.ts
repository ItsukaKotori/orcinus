// DEBUG(replay-paint): remove after diagnosis — console is unreadable during
// headed WKWebView automation, so breadcrumb lines are mirrored to an on-screen
// overlay that survives screenshots.
export function replayPaintDebugLog(message: string): void {
  console.debug(`[replay-paint] ${message}`)
  try {
    const doc = (globalThis as { document?: Document }).document
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
    }
    box.textContent += `${new Date().toISOString().slice(17, 23)} ${message}\n`
  } catch {
    // Overlay is best-effort.
  }
}
