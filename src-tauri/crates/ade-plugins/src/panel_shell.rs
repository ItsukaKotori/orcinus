//! Host panel shell: the CSP/navigation prelude wrapped around plugin panel
//! and main-tab entry HTML before it is handed to the sandboxed iframe.
//!
//! Ported from `orca:src/shared/plugins/plugin-panel-shell.ts`. The shell's
//! job is to make the CSP parse BEFORE any plugin content does: an
//! opaque-origin sandboxed iframe without a CSP can still `fetch()`
//! CORS-permissive endpoints and beacon data out via `<img>`. Prepending
//! works because a CSP `<meta>` applies from the moment it parses and cannot
//! be un-applied by later markup, and a second CSP meta from the plugin can
//! only intersect (tighten), never loosen.
//!
//! The renderer substitutes [`PANEL_SHELL_COLOR_SCHEME_PLACEHOLDER`] and
//! [`PANEL_SHELL_TOKENS_PLACEHOLDER`] before mounting the srcdoc; main cannot
//! know the live theme or computed token values. The inline ping responder
//! (`orca-panel-ping` -> `orca-panel-pong`, echoing `pingId`) proves the
//! frame's event loop is alive; the renderer watchdog demotes the panel when
//! pongs stop arriving.

/// Content-Security-Policy baked into every shell. The first five directives
/// are the frozen contract; the trailing ones keep the design-token `<style>`
/// usable and match the fork renderer's shell expectations.
pub const PLUGIN_PANEL_CSP: &str =
    "default-src 'none'; connect-src 'none'; script-src 'unsafe-inline'; \
     img-src data:; font-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";

/// Renderer-substituted token-declaration placeholder (inside `<style>`).
pub const PANEL_SHELL_TOKENS_PLACEHOLDER: &str = "/*__ORCA_PANEL_TOKENS__*/";
/// Renderer-substituted color-scheme placeholder (the `<html>` class).
pub const PANEL_SHELL_COLOR_SCHEME_PLACEHOLDER: &str = "__ORCA_COLOR_SCHEME__";
/// Renderer watchdog ping message type.
pub const PANEL_PING_TYPE: &str = "orca-panel-ping";
/// Shell pong reply message type.
pub const PANEL_PONG_TYPE: &str = "orca-panel-pong";

/// Curated design-token subset the renderer snapshots from the live document.
pub const PANEL_DESIGN_TOKEN_ALLOWLIST: [&str; 20] = [
    "--background",
    "--foreground",
    "--card",
    "--card-foreground",
    "--popover",
    "--popover-foreground",
    "--primary",
    "--primary-foreground",
    "--secondary",
    "--secondary-foreground",
    "--muted",
    "--muted-foreground",
    "--accent",
    "--accent-foreground",
    "--destructive",
    "--destructive-foreground",
    "--border",
    "--input",
    "--ring",
    "--radius",
];

/// Wraps `entry_html` in the host shell. The plugin document merges into the
/// shell's open `<head>`/`<html>`, so design tokens defined here are visible
/// to plugin CSS as ordinary custom properties.
pub fn build_panel_shell_html(entry_html: &str) -> String {
    let prelude = format!(
        "<!doctype html>\n\
         <html class=\"{PANEL_SHELL_COLOR_SCHEME_PLACEHOLDER}\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{PLUGIN_PANEL_CSP}\">\n\
         <style>:root{{{PANEL_SHELL_TOKENS_PLACEHOLDER}}}</style>\n\
         <script>\n\
         'use strict'\n\
         // Host policy: plugin panels are documents, never browsing contexts.\n\
         if (window.navigation && typeof window.navigation.addEventListener === 'function') {{\n\
         \x20 window.navigation.addEventListener('navigate', function (event) {{\n\
         \x20   if (event.cancelable) event.preventDefault()\n\
         \x20 }})\n\
         }}\n\
         try {{ Object.defineProperty(window, 'open', {{ value: function () {{ return null }}, writable: false, configurable: false }}) }}\n\
         catch (_) {{ try {{ window.open = function () {{ return null }} }} catch (_) {{}} }}\n\
         window.addEventListener('click', function (event) {{\n\
         \x20 var node = event.target\n\
         \x20 while (node && node !== document) {{\n\
         \x20   if (node.nodeType === 1 && node.tagName === 'A' && node.hasAttribute('href')) {{\n\
         \x20     event.preventDefault()\n\
         \x20     event.stopImmediatePropagation()\n\
         \x20     return\n\
         \x20   }}\n\
         \x20   node = node.parentNode\n\
         \x20 }}\n\
         }}, true)\n\
         window.addEventListener('submit', function (event) {{\n\
         \x20 event.preventDefault()\n\
         \x20 event.stopImmediatePropagation()\n\
         }}, true)\n\
         window.addEventListener('message', function (event) {{\n\
         \x20 var data = event.data\n\
         \x20 if (event.source === window.parent && data && data.type === '{PANEL_PING_TYPE}') {{\n\
         \x20   window.parent.postMessage({{ type: '{PANEL_PONG_TYPE}', pingId: data.pingId }}, '*')\n\
         \x20 }}\n\
         }})\n\
         </script>\n\
         </head>\n"
    );
    prelude + entry_html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_carries_the_frozen_directives() {
        assert!(
            PLUGIN_PANEL_CSP.contains(
                "default-src 'none'; connect-src 'none'; script-src 'unsafe-inline'; \
                 img-src data:; font-src data:"
            ),
            "{PLUGIN_PANEL_CSP}"
        );
        assert!(PLUGIN_PANEL_CSP.contains("style-src 'unsafe-inline'"));
        assert!(PLUGIN_PANEL_CSP.contains("base-uri 'none'"));
        assert!(PLUGIN_PANEL_CSP.contains("form-action 'none'"));
    }

    #[test]
    fn prelude_guards_navigation_open_click_and_submit_before_content() {
        let html = build_panel_shell_html("<main id=\"plugin-content\">Plugin</main>");
        let content = html.find("plugin-content").expect("content is appended");
        for marker in [
            "Content-Security-Policy",
            "window.navigation.addEventListener('navigate'",
            "window.addEventListener('click'",
            "window.addEventListener('submit'",
            "Object.defineProperty(window, 'open'",
        ] {
            let offset = html
                .find(marker)
                .unwrap_or_else(|| panic!("missing {marker}"));
            assert!(
                offset < content,
                "{marker} must parse before plugin content"
            );
        }
        assert!(html.ends_with("<main id=\"plugin-content\">Plugin</main>"));
        assert!(html.starts_with("<!doctype html>\n<html class=\"__ORCA_COLOR_SCHEME__\">"));
    }

    #[test]
    fn shell_installs_the_ping_to_pong_responder_with_id_echo() {
        let html = build_panel_shell_html("");
        assert!(html.contains("data.type === 'orca-panel-ping'"));
        assert!(html.contains(
            "window.parent.postMessage({ type: 'orca-panel-pong', pingId: data.pingId }, '*')"
        ));
    }

    #[test]
    fn renderer_placeholders_are_emitted_verbatim() {
        let html = build_panel_shell_html("");
        assert!(html.contains("__ORCA_COLOR_SCHEME__"));
        assert!(html.contains("<style>:root{/*__ORCA_PANEL_TOKENS__*/}</style>"));
        assert_eq!(PANEL_DESIGN_TOKEN_ALLOWLIST.len(), 20);
        assert!(PANEL_DESIGN_TOKEN_ALLOWLIST.contains(&"--destructive-foreground"));
    }
}
