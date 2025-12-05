use cocoa::appkit::{NSWindow, NSWindowStyleMask, NSBackingStoreType};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSRect, NSPoint, NSSize, NSString};
use core_graphics::display::CGDisplay;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, YES, NO};
use objc::{msg_send, sel, sel_impl, class};
use crate::config::Config;
use std::sync::Once;

static WINDOW_CLASS_INIT: Once = Once::new();

// Create a custom window class that can become key (receive keyboard input)
fn create_borderless_window_class() -> *const Class {
    unsafe {
        WINDOW_CLASS_INIT.call_once(|| {
            let superclass = class!(NSWindow);
            let mut decl = ClassDecl::new("BorderlessKeyWindow", superclass).unwrap();

            extern "C" fn can_become_key(_: &Object, _: Sel) -> u8 {
                YES as u8
            }
            extern "C" fn can_become_main(_: &Object, _: Sel) -> u8 {
                YES as u8
            }

            unsafe {
                decl.add_method(sel!(canBecomeKeyWindow), can_become_key as extern "C" fn(&Object, Sel) -> u8);
                decl.add_method(sel!(canBecomeMainWindow), can_become_main as extern "C" fn(&Object, Sel) -> u8);
            }

            decl.register();
        });

        Class::get("BorderlessKeyWindow").unwrap()
    }
}

pub struct RofiWindow {
    pub window: id,
}

impl RofiWindow {
    pub fn new(config: &Config) -> Self {
        unsafe {
            // Get main display dimensions for centering
            let display = CGDisplay::main();
            let screen_width = display.pixels_wide() as f64;
            let screen_height = display.pixels_high() as f64;

            // Calculate centered position
            let x = (screen_width - config.window.width as f64) / 2.0;
            let y = (screen_height - config.window.height as f64) / 2.0;

            let frame = NSRect::new(
                NSPoint::new(x, y),
                NSSize::new(config.window.width as f64, config.window.height as f64),
            );

            // Create custom borderless window that can receive keyboard input
            let window_class = create_borderless_window_class();
            let window: id = msg_send![window_class, alloc];
            let window = window.initWithContentRect_styleMask_backing_defer_(
                frame,
                NSWindowStyleMask::NSBorderlessWindowMask,
                NSBackingStoreType::NSBackingStoreBuffered,
                NO,
            );

            // Configure window properties - use normal level for keyboard input
            let _: () = msg_send![window, setLevel: 0]; // Normal level to receive keyboard
            let _: () = msg_send![window, setOpaque: YES]; // Solid background
            let _: () = msg_send![window, setHasShadow: YES];
            let _: () = msg_send![window, setMovableByWindowBackground: YES];
            let _: () = msg_send![window, setAcceptsMouseMovedEvents: YES];

            // Modern 2026 UI: Rounded corners
            let content_view: id = msg_send![window, contentView];
            let _: () = msg_send![content_view, setWantsLayer: YES];
            let layer: id = msg_send![content_view, layer];
            let _: () = msg_send![layer, setCornerRadius: 16.0f64];
            let _: () = msg_send![layer, setMasksToBounds: YES];

            // Set solid dark background color
            let bg_color = config.get_bg_color();
            let _: () = msg_send![window, setBackgroundColor: bg_color];
            let _: () = msg_send![content_view, setBackgroundColor: bg_color];

            // Make window the key window (will accept keyboard events)
            let _: () = msg_send![window, makeKeyWindow];

            // Make window key and visible
            let _: () = msg_send![window, makeKeyAndOrderFront: nil];
            let _: () = msg_send![window, center];
            let _: () = msg_send![window, orderFrontRegardless];

            // Activate the app
            use cocoa::appkit::NSApp;
            let app = NSApp();
            let _: () = msg_send![app, activateIgnoringOtherApps: 1u32];

            RofiWindow { window }
        }
    }

    pub fn close(&self) {
        unsafe {
            let _: () = msg_send![self.window, close];
        }
    }
}

impl Drop for RofiWindow {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.window, release];
        }
    }
}
