mod app_search;
mod config;
mod delegate;
mod file_search;
mod search_mode;
mod system_commands;
mod ui;
mod window;

use clap::Parser;
use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyRegular};
use objc::{msg_send, sel, sel_impl};
use std::sync::Once;

static INIT: Once = Once::new();

/// Rufi - A minimal macOS application launcher with gruvbox theme
#[derive(Parser, Debug)]
#[command(name = "rufi")]
#[command(about = "A minimal macOS application launcher", long_about = None)]
struct Args {
    /// Window width (overrides config)
    #[arg(short = 'w', long)]
    width: Option<u32>,

    /// Window height (overrides config)
    #[arg(long)]
    height: Option<u32>,

    /// Font size (overrides config)
    #[arg(short = 's', long)]
    font_size: Option<f64>,

    /// Font family (overrides config)
    #[arg(short = 'f', long)]
    font_family: Option<String>,

    /// Background color (hex, e.g., #282828)
    #[arg(long)]
    bg_color: Option<String>,

    /// Text color (hex, e.g., #ebdbb2)
    #[arg(long)]
    text_color: Option<String>,

    /// Selection background color (hex, e.g., #d79921)
    #[arg(long)]
    selection_color: Option<String>,

    /// Theme (gruvbox, 8bit, catppuccin)
    #[arg(short = 't', long)]
    theme: Option<String>,
}

fn main() {
    let args = Args::parse();

    unsafe {
        let app = NSApp();

        INIT.call_once(|| {
            app.setActivationPolicy_(NSApplicationActivationPolicyRegular);

            // Set app delegate
            let delegate = delegate::create_app_delegate();
            let _: () = msg_send![app, setDelegate: delegate];
        });

        // Load config and apply CLI overrides
        let mut config = config::Config::load();

        // Apply CLI overrides
        if let Some(width) = args.width {
            config.window.width = width;
        }
        if let Some(height) = args.height {
            config.window.height = height;
        }
        if let Some(font_size) = args.font_size {
            config.font.size = font_size;
        }
        if let Some(font_family) = args.font_family {
            config.font.family = font_family;
        }
        if let Some(bg_color) = args.bg_color {
            config.colors.background = bg_color;
        }
        if let Some(text_color) = args.text_color {
            config.colors.text = text_color;
        }
        if let Some(selection_color) = args.selection_color {
            config.colors.selection_background = selection_color;
        }
        if let Some(theme) = args.theme {
            config = match theme.as_str() {
                "8bit" => config::Config::theme_8bit(),
                "catppuccin" => config::Config::theme_catppuccin(),
                "modern" => config::Config::theme_modern(),
                _ => config::Config::theme_gruvbox(),
            };
        }

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
