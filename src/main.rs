mod window;
mod app_search;
mod config;
mod ui;
mod delegate;
mod search_mode;
mod file_search;
mod system_commands;

use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyRegular};
use objc::{msg_send, sel, sel_impl};
use std::sync::Once;

static INIT: Once = Once::new();

fn main() {
    unsafe {
        let app = NSApp();

        INIT.call_once(|| {
            app.setActivationPolicy_(NSApplicationActivationPolicyRegular);

            // Set app delegate
            let delegate = delegate::create_app_delegate();
            let _: () = msg_send![app, setDelegate: delegate];
        });

        // Load config
        let config = config::Config::load();

        // Index applications (do this first before creating window)
        println!("Indexing applications...");
        let apps = app_search::index_applications();
        println!("Found {} apps", apps.len());

        // Create borderless window
        let window = window::RofiWindow::new(&config);

        // Create UI
        let _ui = ui::RofiUI::new(window.window, apps, config);

        // Prevent window from being dropped
        std::mem::forget(window);

        // Activate and run
        let _: () = msg_send![app, activateIgnoringOtherApps: 1u32];

        println!("Running app...");
        app.run();
    }
}
