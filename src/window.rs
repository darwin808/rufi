use crate::config::Config;
use cocoa::appkit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use core_graphics::display::CGDisplay;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, NO, YES};
use objc::{class, msg_send, sel, sel_impl};
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
                decl.add_method(
                    sel!(canBecomeKeyWindow),
                    can_become_key as extern "C" fn(&Object, Sel) -> u8,
                );
                decl.add_method(
                    sel!(canBecomeMainWindow),
                    can_become_main as extern "C" fn(&Object, Sel) -> u8,
                );
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
            // Get main display dimensions and calculate window size for 5x3 grid
            let display = CGDisplay::main();
            let screen_width = display.pixels_wide() as f64;
            let screen_height = display.pixels_high() as f64;

            // Calculate width to fit 5 columns: 5 cells(140px each) + 4 gaps(12px) + padding(48px) = ~796px
            let min_width: f64 = 800.0;
            let window_width = min_width.max(screen_width / 2.5);
            // Height calculation: search(60) + 3 rows(140*3) + padding(80) = ~560px
            let min_height: f64 = 60.0 + (140.0 * 3.0) + 80.0;
            let window_height = min_height.max(screen_height / 3.0);

            // Calculate centered position (offset 20px lower)
            let x = (screen_width - window_width) / 2.0;
            let y = (screen_height - window_height) / 2.0 - 20.0;

            let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(window_width, window_height));

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
            let _: () = msg_send![window, setOpaque: NO]; // Transparent for modern effects
            let _: () = msg_send![window, setHasShadow: YES];
            let _: () = msg_send![window, setMovableByWindowBackground: YES];
            let _: () = msg_send![window, setAcceptsMouseMovedEvents: YES];

            // Modern rounded corners (12px for 2025 aesthetic)
            let content_view: id = msg_send![window, contentView];
            let _: () = msg_send![content_view, setWantsLayer: YES];
            let layer: id = msg_send![content_view, layer];
            let _: () = msg_send![layer, setCornerRadius: 16.0f64]; // Larger, more modern
            let _: () = msg_send![layer, setMasksToBounds: YES];

            // Transparent background for glassmorphism
            let cls = class!(NSColor);
            let clear_color: id = msg_send![cls, clearColor];
            let _: () = msg_send![window, setBackgroundColor: clear_color];

            // Semi-transparent background with slight blur effect
            let bg_color = config.get_bg_color();
            let alpha_bg: id = msg_send![bg_color, colorWithAlphaComponent: 0.95f64];
            let _: () = msg_send![content_view, setBackgroundColor: alpha_bg];

            // Make window the key window (will accept keyboard events)
            let _: () = msg_send![window, makeKeyWindow];

            // Make window key and visible
            let _: () = msg_send![window, makeKeyAndOrderFront: nil];
            // Position window: center horizontally, slightly below center vertically
            let origin = NSPoint::new(x, y);
            let _: () = msg_send![window, setFrameOrigin: origin];
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
