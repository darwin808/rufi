mod app_search;
mod config;
mod delegate;
mod file_search;
mod search_mode;
mod system_commands;
pub mod ui;
pub mod window;

use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyRegular};
use lexopt::prelude::*;
use objc::{msg_send, sel, sel_impl};
use std::sync::Once;

static INIT: Once = Once::new();

struct Args {
    width: Option<u32>,
    height: Option<u32>,
    font_size: Option<f64>,
    font_family: Option<String>,
    bg_color: Option<String>,
    text_color: Option<String>,
    selection_color: Option<String>,
    theme: Option<String>,
}

fn parse_args() -> Result<Args, lexopt::Error> {
    let mut args = Args {
        width: None,
        height: None,
        font_size: None,
        font_family: None,
        bg_color: None,
        text_color: None,
        selection_color: None,
        theme: None,
    };

    let mut parser = lexopt::Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Short('w') | Long("width") => args.width = Some(parser.value()?.parse()?),
            Long("height") => args.height = Some(parser.value()?.parse()?),
            Short('s') | Long("font-size") => args.font_size = Some(parser.value()?.parse()?),
            Short('f') | Long("font-family") => args.font_family = Some(parser.value()?.parse()?),
            Long("bg-color") => args.bg_color = Some(parser.value()?.parse()?),
            Long("text-color") => args.text_color = Some(parser.value()?.parse()?),
            Long("selection-color") => args.selection_color = Some(parser.value()?.parse()?),
            Short('t') | Long("theme") => args.theme = Some(parser.value()?.parse()?),
            Long("help") => {
                eprintln!("rufi - A minimal macOS application launcher\n");
                eprintln!("USAGE: rufi [OPTIONS]\n");
                eprintln!("OPTIONS:");
                eprintln!("  -w, --width <WIDTH>         Window width");
                eprintln!("      --height <HEIGHT>       Window height");
                eprintln!("  -s, --font-size <SIZE>      Font size");
                eprintln!("  -f, --font-family <FONT>    Font family");
                eprintln!("      --bg-color <HEX>        Background color");
                eprintln!("      --text-color <HEX>      Text color");
                eprintln!("      --selection-color <HEX> Selection color");
                eprintln!("  -t, --theme <THEME>         Theme (gruvbox, 8bit, catppuccin, modern)");
                eprintln!("      --help                  Show this help");
                std::process::exit(0);
            }
            _ => return Err(arg.unexpected()),
        }
    }
    Ok(args)
}

fn main() {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    });

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
        let apps = app_search::index_applications();

        // Create borderless window
        let window = window::RofiWindow::new(&config);

        // Create UI
        let _ui = ui::RofiUI::new(window.window, apps, config);

        // Prevent window from being dropped
        std::mem::forget(window);

        // Activate and run
        let _: () = msg_send![app, activateIgnoringOtherApps: 1u32];
        app.run();
    }
}
