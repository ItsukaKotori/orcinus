use cef::args::Args;
use cef::*;
use std::cell::RefCell;

#[cfg(target_os = "macos")]
mod macos_application;

const SPIKE_PAGE: &str = "data:text/html,%3Chtml%3E%3Chead%3E%3Ctitle%3Eade%20cef%20spike%3C/title%3E%3C/head%3E%3Cbody%3E%3Ch1%3Eade%20cef%20spike%3C/h1%3E%3C/body%3E%3C/html%3E";

fn main() {
    #[cfg(target_os = "macos")]
    let _library = {
        let loader = library_loader::LibraryLoader::new(
            &std::env::current_exe().expect("current exe path is unavailable"),
            false,
        );
        assert!(loader.load(), "failed to load Chromium Embedded Framework");
        loader
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    #[cfg(target_os = "macos")]
    macos_application::install();

    let args = Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        panic!("failed to parse command line arguments");
    };

    let process_type_switch = CefString::from("type");
    let is_browser_process = cmd_line.has_switch(Some(&process_type_switch)) != 1;
    let execute_result = execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());

    if is_browser_process {
        assert_eq!(
            execute_result, -1,
            "browser process must not execute a subprocess"
        );
    } else {
        return;
    }

    let mut app = SpikeApp::new();
    let settings = Settings {
        no_sandbox: 1,
        ..Default::default()
    };
    assert_eq!(
        initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ),
        1,
        "cef_initialize failed"
    );

    println!("[cef-spike] browser process started");
    run_message_loop();
    shutdown();
}

wrap_app! {
    struct SpikeApp;

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(SpikeBrowserProcessHandler::new(RefCell::new(None)))
        }
    }
}

wrap_browser_process_handler! {
    struct SpikeBrowserProcessHandler {
        client: RefCell<Option<Client>>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let mut client = SpikeClient::new();
            self.client.borrow_mut().replace(client.clone());

            let settings = BrowserSettings::default();
            let url = CefString::from(SPIKE_PAGE);
            let mut view_delegate = SpikeBrowserViewDelegate::new(RuntimeStyle::DEFAULT);
            let browser_view = browser_view_create(
                Some(&mut client),
                Some(&url),
                Some(&settings),
                None,
                None,
                Some(&mut view_delegate),
            );

            let mut window_delegate = SpikeWindowDelegate::new(
                RefCell::new(browser_view),
                RuntimeStyle::DEFAULT,
            );
            window_create_top_level(Some(&mut window_delegate));
        }
    }
}

wrap_client! {
    struct SpikeClient;

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(SpikeDisplayHandler::new())
        }
    }
}

wrap_display_handler! {
    struct SpikeDisplayHandler;

    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            let title = title.map(CefString::to_string).unwrap_or_default();
            println!("[cef-spike] title changed: {title}");
        }
    }
}

wrap_browser_view_delegate! {
    struct SpikeBrowserViewDelegate {
        runtime_style: RuntimeStyle,
    }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}

wrap_window_delegate! {
    struct SpikeWindowDelegate {
        browser_view: RefCell<Option<BrowserView>>,
        runtime_style: RuntimeStyle,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size {
                width: 800,
                height: 600,
            }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let browser_view = self.browser_view.borrow();
            let (Some(window), Some(browser_view)) = (window, browser_view.as_ref()) else {
                return;
            };
            let mut view = View::from(browser_view);
            window.add_child_view(Some(&mut view));
            window.set_title(Some(&CefString::from("ade cef spike")));
            window.show();
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            self.browser_view.borrow_mut().take();
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            ShowState::NORMAL
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}
