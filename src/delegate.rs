use cocoa::base::id;
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};

pub fn create_app_delegate() -> id {
    unsafe {
        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("RofiAppDelegate", superclass).unwrap();

        // Add method to handle app finishing launch
        extern "C" fn did_finish_launching(_: &Object, _: Sel, _: id) {
            // App launched successfully
        }

        // Add method to handle window close
        extern "C" fn should_terminate(_: &Object, _: Sel, _: id) -> i32 {
            1 // NSTerminateNow
        }

        decl.add_method(
            sel!(applicationDidFinishLaunching:),
            did_finish_launching as extern "C" fn(&Object, Sel, id),
        );

        decl.add_method(
            sel!(applicationShouldTerminateAfterLastWindowClosed:),
            should_terminate as extern "C" fn(&Object, Sel, id) -> i32,
        );

        let delegate_class = decl.register();
        let delegate: id = msg_send![delegate_class, new];

        delegate
    }
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}
