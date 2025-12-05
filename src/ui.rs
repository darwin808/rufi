use cocoa::appkit::{NSTextField, NSApp, NSButton};
use cocoa::base::{id, nil, YES, NO};
use cocoa::foundation::{NSRect, NSPoint, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{msg_send, sel, sel_impl, class};
use std::sync::{Arc, Mutex, Once};
use std::collections::HashMap;
use crate::{
    app_search::{Application, fuzzy_search},
    config::Config,
    search_mode::{SearchMode, SearchResult},
    file_search::{search_files, search_files_random},
    system_commands::search_commands,
};

static DELEGATE_CLASS_INIT: Once = Once::new();
static BUTTON_ACTION_CLASS_INIT: Once = Once::new();

// Wrapper for id that implements Send (safe because all access is on main thread)
#[derive(Clone, Copy)]
struct SendId(id);
unsafe impl Send for SendId {}

// Global storage for delegate data
struct DelegateData {
    results_view: SendId,
    apps: Arc<Mutex<Vec<Application>>>,
    filtered: Arc<Mutex<Vec<SearchResult>>>, // Currently filtered/displayed results
    selected_index: Arc<Mutex<usize>>, // Currently selected item index
    search_mode: Arc<Mutex<SearchMode>>, // Current search mode
    search_field: SendId, // Reference to search field for refreshing
    pill_buttons: Vec<SendId>, // References to the 3 pill buttons
}

static DELEGATE_DATA: Mutex<Option<HashMap<usize, DelegateData>>> = Mutex::new(None);

// Create a text field delegate that handles text changes and key commands
fn create_text_field_delegate_class() -> *const Class {
    unsafe {
        DELEGATE_CLASS_INIT.call_once(|| {
            let superclass = class!(NSObject);
            let mut decl = ClassDecl::new("RofiTextFieldDelegate", superclass).unwrap();

            // Handle text changes for real-time filtering
            extern "C" fn control_text_did_change(_this: &Object, _: Sel, notification: id) {
                unsafe {
                    // Get the text field from the notification
                    let text_field: id = msg_send![notification, object];
                    let delegate: id = msg_send![text_field, delegate];
                    let delegate_ptr = delegate as usize;

                    // Get delegate data from global storage
                    let mut data_map = DELEGATE_DATA.lock().unwrap();
                    let data = match data_map.as_mut().and_then(|m| m.get(&delegate_ptr)) {
                        Some(d) => d,
                        None => return,
                    };

                    let text: id = msg_send![text_field, stringValue];
                    let query_cstr: *const i8 = msg_send![text, UTF8String];
                    let query = std::ffi::CStr::from_ptr(query_cstr).to_string_lossy();

                    println!("Search query: {}", query);

                    // Get current search mode
                    let mode = *data.search_mode.lock().unwrap();

                    // Filter based on mode
                    let filtered: Vec<SearchResult> = match mode {
                        SearchMode::Apps => {
                            if query.is_empty() {
                                // Show 4 random apps when empty
                                use rand::seq::SliceRandom;
                                let mut rng = rand::thread_rng();
                                let apps = data.apps.lock().unwrap();
                                let mut app_vec: Vec<_> = apps.iter().collect();
                                app_vec.shuffle(&mut rng);
                                app_vec.into_iter()
                                    .take(4)
                                    .map(|app| SearchResult::new(app.name.clone(), app.path.clone(), SearchMode::Apps))
                                    .collect()
                            } else {
                                fuzzy_search(&data.apps.lock().unwrap(), &query)
                                    .into_iter()
                                    .take(8)
                                    .map(|app| SearchResult::new(app.name, app.path, SearchMode::Apps))
                                    .collect()
                            }
                        }
                        SearchMode::Files => {
                            if query.is_empty() {
                                // Show 4 random files when empty
                                search_files_random(4)
                            } else {
                                search_files(&query)
                            }
                        }
                        SearchMode::Run => {
                            search_commands(&query)
                        }
                    };

                    // Store filtered results and reset selection to first item
                    *data.filtered.lock().unwrap() = filtered.clone();
                    *data.selected_index.lock().unwrap() = 0;

                    // Rebuild the results view with icons
                    let results_view = data.results_view.0;

                    // Remove all existing subviews - get fresh copy each time
                    loop {
                        let subviews: id = msg_send![results_view, subviews];
                        let count: usize = msg_send![subviews, count];
                        if count == 0 {
                            break;
                        }
                        let subview: id = msg_send![subviews, firstObject];
                        let _: () = msg_send![subview, removeFromSuperview];
                    }

                    // Get config colors
                    let selection_bg = Config::hex_to_nscolor("#d946ef");
                    let selection_text = Config::hex_to_nscolor("#ffffff");
                    let normal_text = Config::hex_to_nscolor("#e0e0e0");

                    // Recreate rows for filtered results
                    let workspace_class = class!(NSWorkspace);
                    let workspace: id = msg_send![workspace_class, sharedWorkspace];
                    let row_height = 44.0;
                    let icon_size = 32.0;
                    let frame: NSRect = msg_send![results_view, frame];
                    let container_height = frame.size.height;
                    let selected_idx = *data.selected_index.lock().unwrap();

                    for (index, result) in filtered.iter().enumerate() {
                        let y_pos = container_height - ((index + 1) as f64 * row_height);

                        // Create row
                        let row_frame = NSRect::new(
                            NSPoint::new(0.0, y_pos),
                            NSSize::new(frame.size.width, row_height),
                        );
                        let row_view: id = msg_send![class!(NSView), alloc];
                        let row_view: id = msg_send![row_view, initWithFrame: row_frame];
                        let _: () = msg_send![row_view, setWantsLayer: 1u32];

                        // Highlight selected row
                        if index == selected_idx {
                            let row_layer: id = msg_send![row_view, layer];
                            let cg_color: id = msg_send![selection_bg, CGColor];
                            let _: () = msg_send![row_layer, setBackgroundColor: cg_color];
                            let _: () = msg_send![row_layer, setCornerRadius: 8.0f64];
                        }

                        // Icon (only for Apps and Files)
                        if result.result_type == SearchMode::Apps || result.result_type == SearchMode::Files {
                            let path_str = NSString::alloc(nil).init_str(&result.path);
                            let icon: id = msg_send![workspace, iconForFile: path_str];
                            let icon_frame = NSRect::new(
                                NSPoint::new(12.0, (row_height - icon_size) / 2.0),
                                NSSize::new(icon_size, icon_size),
                            );
                            let icon_view: id = msg_send![class!(NSImageView), alloc];
                            let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
                            let _: () = msg_send![icon_view, setImage: icon];
                            let _: () = msg_send![row_view, addSubview: icon_view];
                        }

                        // Label
                        let label_frame = NSRect::new(
                            NSPoint::new(56.0, (row_height - 20.0) / 2.0),
                            NSSize::new(frame.size.width - 68.0, 20.0),
                        );
                        let label: id = msg_send![class!(NSTextField), alloc];
                        let label: id = msg_send![label, initWithFrame: label_frame];
                        let _: () = msg_send![label, setEditable: 0u32];
                        let _: () = msg_send![label, setSelectable: 0u32];
                        let _: () = msg_send![label, setBordered: 0u32];
                        let _: () = msg_send![label, setDrawsBackground: 0u32];
                        let text_color = if index == selected_idx { selection_text } else { normal_text };
                        let _: () = msg_send![label, setTextColor: text_color];
                        let font_cls = class!(NSFont);
                        let font: id = msg_send![font_cls, systemFontOfSize: 15.0f64];
                        let _: () = msg_send![label, setFont: font];
                        let name_str = NSString::alloc(nil).init_str(&result.name);
                        let _: () = msg_send![label, setStringValue: name_str];

                        let _: () = msg_send![row_view, addSubview: label];
                        let _: () = msg_send![results_view, addSubview: row_view];
                    }
                }
            }

            // Handle command keys (Escape, Enter)
            extern "C" fn control_text_view_do_command_by_selector(
                this: &Object,
                _: Sel,
                control: id,
                _text_view: id,
                command_selector: Sel,
            ) -> u8 {
                unsafe {
                    extern "C" {
                        fn sel_getName(sel: Sel) -> *const i8;
                    }

                    let sel_name = sel_getName(command_selector);
                    let sel_str = std::ffi::CStr::from_ptr(sel_name).to_string_lossy();

                    // Escape key triggers "cancelOperation:"
                    if sel_str == "cancelOperation:" {
                        let app = NSApp();
                        let _: () = msg_send![app, terminate: nil];
                        return YES as u8;
                    }

                    // Enter/Return triggers "insertNewline:"
                    if sel_str == "insertNewline:" {
                        // Get delegate data
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_ref().and_then(|m| m.get(&delegate_ptr)) {
                            let filtered = data.filtered.lock().unwrap();
                            let selected_idx = *data.selected_index.lock().unwrap();
                            if let Some(result) = filtered.get(selected_idx) {
                                println!("Launching: {} (type: {:?})", result.name, result.result_type);

                                match result.result_type {
                                    SearchMode::Apps | SearchMode::Files => {
                                        // Launch application or open file using NSWorkspace
                                        let workspace_class = class!(NSWorkspace);
                                        let workspace: id = msg_send![workspace_class, sharedWorkspace];
                                        let path_string = NSString::alloc(nil).init_str(&result.path);
                                        let _: id = msg_send![workspace, openFile: path_string];
                                    }
                                    SearchMode::Run => {
                                        // Execute system command
                                        std::process::Command::new("sh")
                                            .arg("-c")
                                            .arg(&result.path) // path contains the command
                                            .spawn()
                                            .ok();
                                    }
                                }

                                // Close rofi-mac after launching
                                let app = NSApp();
                                let _: () = msg_send![app, terminate: nil];
                            }
                        }

                        return YES as u8;
                    }

                    // Arrow Down triggers "moveDown:"
                    if sel_str == "moveDown:" {
                        println!("Arrow Down pressed");
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr)) {
                            let filtered_count = data.filtered.lock().unwrap().len();
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx < filtered_count.saturating_sub(1) {
                                *selected_idx += 1;
                                println!("Selection moved to: {}", *selected_idx);
                            }
                            drop(selected_idx);

                            // Manually rebuild UI
                            let results_view = data.results_view.0;
                            let filtered = data.filtered.lock().unwrap().clone();
                            let selected_index = *data.selected_index.lock().unwrap();
                            drop(data_map);

                            // Clear subviews
                            loop {
                                let subviews: id = msg_send![results_view, subviews];
                                let count: usize = msg_send![subviews, count];
                                if count == 0 { break; }
                                let subview: id = msg_send![subviews, firstObject];
                                let _: () = msg_send![subview, removeFromSuperview];
                            }

                            // Rebuild rows
                            let selection_bg = Config::hex_to_nscolor("#d946ef");
                            let selection_text = Config::hex_to_nscolor("#ffffff");
                            let normal_text = Config::hex_to_nscolor("#e0e0e0");
                            let workspace_class = class!(NSWorkspace);
                            let workspace: id = msg_send![workspace_class, sharedWorkspace];
                            let row_height = 44.0;
                            let icon_size = 32.0;
                            let frame: NSRect = msg_send![results_view, frame];
                            let container_height = frame.size.height;

                            for (index, result) in filtered.iter().enumerate() {
                                let y_pos = container_height - ((index + 1) as f64 * row_height);
                                let row_frame = NSRect::new(NSPoint::new(0.0, y_pos), NSSize::new(frame.size.width, row_height));
                                let row_view: id = msg_send![class!(NSView), alloc];
                                let row_view: id = msg_send![row_view, initWithFrame: row_frame];
                                let _: () = msg_send![row_view, setWantsLayer: 1u32];

                                if index == selected_index {
                                    let row_layer: id = msg_send![row_view, layer];
                                    let cg_color: id = msg_send![selection_bg, CGColor];
                                    let _: () = msg_send![row_layer, setBackgroundColor: cg_color];
                                    let _: () = msg_send![row_layer, setCornerRadius: 8.0f64];
                                }

                                if result.result_type == SearchMode::Apps || result.result_type == SearchMode::Files {
                                    let path_str = NSString::alloc(nil).init_str(&result.path);
                                    let icon: id = msg_send![workspace, iconForFile: path_str];
                                    let icon_frame = NSRect::new(NSPoint::new(12.0, (row_height - icon_size) / 2.0), NSSize::new(icon_size, icon_size));
                                    let icon_view: id = msg_send![class!(NSImageView), alloc];
                                    let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
                                    let _: () = msg_send![icon_view, setImage: icon];
                                    let _: () = msg_send![row_view, addSubview: icon_view];
                                }

                                let label_frame = NSRect::new(NSPoint::new(56.0, (row_height - 20.0) / 2.0), NSSize::new(frame.size.width - 68.0, 20.0));
                                let label: id = msg_send![class!(NSTextField), alloc];
                                let label: id = msg_send![label, initWithFrame: label_frame];
                                let _: () = msg_send![label, setEditable: 0u32];
                                let _: () = msg_send![label, setSelectable: 0u32];
                                let _: () = msg_send![label, setBordered: 0u32];
                                let _: () = msg_send![label, setDrawsBackground: 0u32];
                                let text_color = if index == selected_index { selection_text } else { normal_text };
                                let _: () = msg_send![label, setTextColor: text_color];
                                let font_cls = class!(NSFont);
                                let font: id = msg_send![font_cls, systemFontOfSize: 15.0f64];
                                let _: () = msg_send![label, setFont: font];
                                let name_str = NSString::alloc(nil).init_str(&result.name);
                                let _: () = msg_send![label, setStringValue: name_str];
                                let _: () = msg_send![row_view, addSubview: label];
                                let _: () = msg_send![results_view, addSubview: row_view];
                            }
                        }
                        return YES as u8;
                    }

                    // Arrow Up triggers "moveUp:"
                    if sel_str == "moveUp:" {
                        println!("Arrow Up pressed");
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr)) {
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx > 0 {
                                *selected_idx -= 1;
                                println!("Selection moved to: {}", *selected_idx);
                            }
                            drop(selected_idx);

                            // Manually rebuild UI
                            let results_view = data.results_view.0;
                            let filtered = data.filtered.lock().unwrap().clone();
                            let selected_index = *data.selected_index.lock().unwrap();
                            drop(data_map);

                            // Clear subviews
                            loop {
                                let subviews: id = msg_send![results_view, subviews];
                                let count: usize = msg_send![subviews, count];
                                if count == 0 { break; }
                                let subview: id = msg_send![subviews, firstObject];
                                let _: () = msg_send![subview, removeFromSuperview];
                            }

                            // Rebuild rows
                            let selection_bg = Config::hex_to_nscolor("#d946ef");
                            let selection_text = Config::hex_to_nscolor("#ffffff");
                            let normal_text = Config::hex_to_nscolor("#e0e0e0");
                            let workspace_class = class!(NSWorkspace);
                            let workspace: id = msg_send![workspace_class, sharedWorkspace];
                            let row_height = 44.0;
                            let icon_size = 32.0;
                            let frame: NSRect = msg_send![results_view, frame];
                            let container_height = frame.size.height;

                            for (index, result) in filtered.iter().enumerate() {
                                let y_pos = container_height - ((index + 1) as f64 * row_height);
                                let row_frame = NSRect::new(NSPoint::new(0.0, y_pos), NSSize::new(frame.size.width, row_height));
                                let row_view: id = msg_send![class!(NSView), alloc];
                                let row_view: id = msg_send![row_view, initWithFrame: row_frame];
                                let _: () = msg_send![row_view, setWantsLayer: 1u32];

                                if index == selected_index {
                                    let row_layer: id = msg_send![row_view, layer];
                                    let cg_color: id = msg_send![selection_bg, CGColor];
                                    let _: () = msg_send![row_layer, setBackgroundColor: cg_color];
                                    let _: () = msg_send![row_layer, setCornerRadius: 8.0f64];
                                }

                                if result.result_type == SearchMode::Apps || result.result_type == SearchMode::Files {
                                    let path_str = NSString::alloc(nil).init_str(&result.path);
                                    let icon: id = msg_send![workspace, iconForFile: path_str];
                                    let icon_frame = NSRect::new(NSPoint::new(12.0, (row_height - icon_size) / 2.0), NSSize::new(icon_size, icon_size));
                                    let icon_view: id = msg_send![class!(NSImageView), alloc];
                                    let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
                                    let _: () = msg_send![icon_view, setImage: icon];
                                    let _: () = msg_send![row_view, addSubview: icon_view];
                                }

                                let label_frame = NSRect::new(NSPoint::new(56.0, (row_height - 20.0) / 2.0), NSSize::new(frame.size.width - 68.0, 20.0));
                                let label: id = msg_send![class!(NSTextField), alloc];
                                let label: id = msg_send![label, initWithFrame: label_frame];
                                let _: () = msg_send![label, setEditable: 0u32];
                                let _: () = msg_send![label, setSelectable: 0u32];
                                let _: () = msg_send![label, setBordered: 0u32];
                                let _: () = msg_send![label, setDrawsBackground: 0u32];
                                let text_color = if index == selected_index { selection_text } else { normal_text };
                                let _: () = msg_send![label, setTextColor: text_color];
                                let font_cls = class!(NSFont);
                                let font: id = msg_send![font_cls, systemFontOfSize: 15.0f64];
                                let _: () = msg_send![label, setFont: font];
                                let name_str = NSString::alloc(nil).init_str(&result.name);
                                let _: () = msg_send![label, setStringValue: name_str];
                                let _: () = msg_send![row_view, addSubview: label];
                                let _: () = msg_send![results_view, addSubview: row_view];
                            }
                        }
                        return YES as u8;
                    }

                    NO as u8
                }
            }

            unsafe {
                decl.add_method(
                    sel!(controlTextDidChange:),
                    control_text_did_change as extern "C" fn(&Object, Sel, id),
                );

                decl.add_method(
                    sel!(control:textView:doCommandBySelector:),
                    control_text_view_do_command_by_selector as extern "C" fn(&Object, Sel, id, id, Sel) -> u8,
                );
            }

            decl.register();
        });

        Class::get("RofiTextFieldDelegate").unwrap()
    }
}

// Create a button action class for pill button clicks
fn create_button_action_class() -> *const Class {
    unsafe {
        BUTTON_ACTION_CLASS_INIT.call_once(|| {
            let superclass = class!(NSObject);
            let mut decl = ClassDecl::new("PillButtonAction", superclass).unwrap();

            extern "C" fn button_clicked(this: &Object, _: Sel, button: id) {
                unsafe {
                    // Get the button's tag to determine which mode was selected
                    let tag: isize = msg_send![button, tag];
                    let new_mode = match tag {
                        0 => SearchMode::Apps,
                        1 => SearchMode::Files,
                        2 => SearchMode::Run,
                        _ => SearchMode::Apps,
                    };

                    println!("Pill button clicked: {:?}", new_mode);

                    // Get the button's superview (container) to find the search field
                    let superview: id = msg_send![button, superview];
                    let window: id = msg_send![superview, window];
                    let content_view: id = msg_send![window, contentView];

                    // Find the text field in the content view
                    let subviews: id = msg_send![content_view, subviews];
                    let count: usize = msg_send![subviews, count];
                    let mut text_field: id = nil;
                    for i in 0..count {
                        let view: id = msg_send![subviews, objectAtIndex: i];
                        let class_name: id = msg_send![view, className];
                        let cstr: *const i8 = msg_send![class_name, UTF8String];
                        let name = std::ffi::CStr::from_ptr(cstr).to_string_lossy();
                        if name == "NSTextField" {
                            text_field = view;
                            break;
                        }
                    }

                    if text_field != nil {
                        let delegate: id = msg_send![text_field, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr)) {
                            // Update search mode
                            *data.search_mode.lock().unwrap() = new_mode;

                            // Update pill button styles
                            for (idx, pill_btn) in data.pill_buttons.iter().enumerate() {
                                let btn = pill_btn.0;
                                if idx == tag as usize {
                                    // Active pill - magenta background
                                    let active_color = Config::hex_to_nscolor("#d946ef");
                                    let _: () = msg_send![btn, setBackgroundColor: active_color];
                                } else {
                                    // Inactive pill - dark background
                                    let inactive_color = Config::hex_to_nscolor("#2a2640");
                                    let _: () = msg_send![btn, setBackgroundColor: inactive_color];
                                }
                            }

                            // Trigger a search with the current query
                            let text: id = msg_send![text_field, stringValue];
                            let _: () = msg_send![text_field, setStringValue: text]; // Trigger text change event

                            // Manually trigger search
                            let query_cstr: *const i8 = msg_send![text, UTF8String];
                            let query = std::ffi::CStr::from_ptr(query_cstr).to_string_lossy();

                            let filtered: Vec<SearchResult> = match new_mode {
                                SearchMode::Apps => {
                                    if query.is_empty() {
                                        // Show 4 random apps when empty
                                        use rand::seq::SliceRandom;
                                        let mut rng = rand::thread_rng();
                                        let apps = data.apps.lock().unwrap();
                                        let mut app_vec: Vec<_> = apps.iter().collect();
                                        app_vec.shuffle(&mut rng);
                                        app_vec.into_iter()
                                            .take(4)
                                            .map(|app| SearchResult::new(app.name.clone(), app.path.clone(), SearchMode::Apps))
                                            .collect()
                                    } else {
                                        fuzzy_search(&data.apps.lock().unwrap(), &query)
                                            .into_iter()
                                            .take(8)
                                            .map(|app| SearchResult::new(app.name, app.path, SearchMode::Apps))
                                            .collect()
                                    }
                                }
                                SearchMode::Files => {
                                    if query.is_empty() {
                                        search_files_random(4)
                                    } else {
                                        search_files(&query)
                                    }
                                }
                                SearchMode::Run => search_commands(&query),
                            };

                            *data.filtered.lock().unwrap() = filtered.clone();
                            *data.selected_index.lock().unwrap() = 0;

                            // Rebuild UI
                            let results_view = data.results_view.0;
                            loop {
                                let subviews: id = msg_send![results_view, subviews];
                                let count: usize = msg_send![subviews, count];
                                if count == 0 { break; }
                                let subview: id = msg_send![subviews, firstObject];
                                let _: () = msg_send![subview, removeFromSuperview];
                            }

                            let selection_bg = Config::hex_to_nscolor("#d946ef");
                            let selection_text = Config::hex_to_nscolor("#ffffff");
                            let normal_text = Config::hex_to_nscolor("#e0e0e0");
                            let workspace_class = class!(NSWorkspace);
                            let workspace: id = msg_send![workspace_class, sharedWorkspace];
                            let row_height = 44.0;
                            let icon_size = 32.0;
                            let frame: NSRect = msg_send![results_view, frame];
                            let container_height = frame.size.height;

                            for (index, result) in filtered.iter().enumerate() {
                                let y_pos = container_height - ((index + 1) as f64 * row_height);
                                let row_frame = NSRect::new(NSPoint::new(0.0, y_pos), NSSize::new(frame.size.width, row_height));
                                let row_view: id = msg_send![class!(NSView), alloc];
                                let row_view: id = msg_send![row_view, initWithFrame: row_frame];
                                let _: () = msg_send![row_view, setWantsLayer: 1u32];

                                if index == 0 {
                                    let row_layer: id = msg_send![row_view, layer];
                                    let cg_color: id = msg_send![selection_bg, CGColor];
                                    let _: () = msg_send![row_layer, setBackgroundColor: cg_color];
                                    let _: () = msg_send![row_layer, setCornerRadius: 8.0f64];
                                }

                                if result.result_type == SearchMode::Apps || result.result_type == SearchMode::Files {
                                    let path_str = NSString::alloc(nil).init_str(&result.path);
                                    let icon: id = msg_send![workspace, iconForFile: path_str];
                                    let icon_frame = NSRect::new(NSPoint::new(12.0, (row_height - icon_size) / 2.0), NSSize::new(icon_size, icon_size));
                                    let icon_view: id = msg_send![class!(NSImageView), alloc];
                                    let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
                                    let _: () = msg_send![icon_view, setImage: icon];
                                    let _: () = msg_send![row_view, addSubview: icon_view];
                                }

                                let label_frame = NSRect::new(NSPoint::new(56.0, (row_height - 20.0) / 2.0), NSSize::new(frame.size.width - 68.0, 20.0));
                                let label: id = msg_send![class!(NSTextField), alloc];
                                let label: id = msg_send![label, initWithFrame: label_frame];
                                let _: () = msg_send![label, setEditable: 0u32];
                                let _: () = msg_send![label, setSelectable: 0u32];
                                let _: () = msg_send![label, setBordered: 0u32];
                                let _: () = msg_send![label, setDrawsBackground: 0u32];
                                let text_color = if index == 0 { selection_text } else { normal_text };
                                let _: () = msg_send![label, setTextColor: text_color];
                                let font_cls = class!(NSFont);
                                let font: id = msg_send![font_cls, systemFontOfSize: 15.0f64];
                                let _: () = msg_send![label, setFont: font];
                                let name_str = NSString::alloc(nil).init_str(&result.name);
                                let _: () = msg_send![label, setStringValue: name_str];
                                let _: () = msg_send![row_view, addSubview: label];
                                let _: () = msg_send![results_view, addSubview: row_view];
                            }
                        }
                    }
                }
            }

            unsafe {
                decl.add_method(
                    sel!(buttonClicked:),
                    button_clicked as extern "C" fn(&Object, Sel, id),
                );
            }

            decl.register();
        });

        Class::get("PillButtonAction").unwrap()
    }
}

pub struct RofiUI {
    _search_field: id,
    _results_view: id,
    _apps: Arc<Mutex<Vec<Application>>>,
    _filtered: Arc<Mutex<Vec<SearchResult>>>,
    _config: Config,
    _window: id,
    _pill_buttons: Vec<id>,
    _search_mode: Arc<Mutex<SearchMode>>,
}

impl RofiUI {
    pub fn new(window: id, apps: Vec<Application>, config: Config) -> Self {
        unsafe {
            let apps = Arc::new(Mutex::new(apps.clone()));
            let filtered = Arc::new(Mutex::new(apps.lock().unwrap().clone()));

            // Modern 2026 UI: Create search field with better spacing
            let search_padding = 24.0;
            let search_height = 44.0;
            let search_frame = NSRect::new(
                NSPoint::new(search_padding, config.window.height as f64 - search_height - search_padding),
                NSSize::new(config.window.width as f64 - (search_padding * 2.0), search_height),
            );

            let search_field_alloc = NSTextField::alloc(nil);
            let search_field: id = msg_send![search_field_alloc, initWithFrame: search_frame];
            let placeholder = NSString::alloc(nil).init_str("Search applications...");
            let _: () = msg_send![search_field, setPlaceholderString: placeholder];
            let _: () = msg_send![search_field, setBezeled: 0u32]; // Remove bezel
            let _: () = msg_send![search_field, setBordered: 0u32]; // Remove border
            let _: () = msg_send![search_field, setEditable: 1u32];
            let _: () = msg_send![search_field, setSelectable: 1u32];
            let _: () = msg_send![search_field, setDrawsBackground: 1u32];
            let _: () = msg_send![search_field, setFocusRingType: 0u32]; // Remove focus ring

            // Modern dark colors
            let input_bg_color = Config::hex_to_nscolor(&config.colors.input_background);
            let _: () = msg_send![search_field, setBackgroundColor: input_bg_color];
            let _: () = msg_send![search_field, setTextColor: config.get_text_color()];

            // Add rounded corners to search field
            let _: () = msg_send![search_field, setWantsLayer: 1u32];
            let search_layer: id = msg_send![search_field, layer];
            let _: () = msg_send![search_layer, setCornerRadius: 10.0f64];
            let _: () = msg_send![search_layer, setMasksToBounds: 1u32];

            // Set font - larger for readability
            let font_cls = class!(NSFont);
            let font: id = msg_send![font_cls, systemFontOfSize: 18.0f64];
            let _: () = msg_send![search_field, setFont: font];

            // Add views to window (now using visual effect view as content view)
            let content_view: id = msg_send![window, contentView];
            let _: () = msg_send![content_view, addSubview: search_field];

            // Create pill buttons below search field
            let pill_padding = 24.0;
            let pill_spacing = 8.0;
            let pill_height = 32.0;
            let pill_y = config.window.height as f64 - search_height - pill_padding - 8.0 - pill_height;
            let pill_width = 80.0;

            let button_action_class = create_button_action_class();
            let button_action: id = msg_send![button_action_class, new];

            let modes = vec!["Apps", "Files", "Run"];
            let mut pill_buttons = Vec::new();

            for (index, mode) in modes.iter().enumerate() {
                let x = pill_padding + (index as f64 * (pill_width + pill_spacing));
                let pill_frame = NSRect::new(
                    NSPoint::new(x, pill_y),
                    NSSize::new(pill_width, pill_height),
                );

                let button: id = msg_send![class!(NSButton), alloc];
                let button: id = msg_send![button, initWithFrame: pill_frame];
                let _: () = msg_send![button, setTitle: NSString::alloc(nil).init_str(mode)];
                let _: () = msg_send![button, setTag: index as isize];
                let _: () = msg_send![button, setTarget: button_action];
                let _: () = msg_send![button, setAction: sel!(buttonClicked:)];
                let _: () = msg_send![button, setBordered: 0u32];
                let _: () = msg_send![button, setWantsLayer: 1u32];

                let button_layer: id = msg_send![button, layer];
                let _: () = msg_send![button_layer, setCornerRadius: 16.0f64];

                // First pill (Apps) is active by default
                if index == 0 {
                    let active_color = Config::hex_to_nscolor("#d946ef");
                    let _: () = msg_send![button, setBackgroundColor: active_color];
                } else {
                    let inactive_color = Config::hex_to_nscolor("#2a2640");
                    let _: () = msg_send![button, setBackgroundColor: inactive_color];
                }

                // Set text color to white
                let text_color = Config::hex_to_nscolor("#ffffff");
                let font_cls = class!(NSFont);
                let font: id = msg_send![font_cls, systemFontOfSize: 13.0f64];
                let _: () = msg_send![button, setFont: font];

                // NSButton text color is set via attributed string
                let title: id = msg_send![button, title];
                let attrs_dict = NSString::alloc(nil).init_str("NSColor");
                let mut_attrs: id = msg_send![class!(NSMutableDictionary), alloc];
                let mut_attrs: id = msg_send![mut_attrs, init];
                let _: () = msg_send![mut_attrs, setObject:text_color forKey:NSString::alloc(nil).init_str("NSForegroundColor")];
                let attr_string: id = msg_send![class!(NSAttributedString), alloc];
                let attr_string: id = msg_send![attr_string, initWithString:title attributes:mut_attrs];
                let _: () = msg_send![button, setAttributedTitle: attr_string];

                let _: () = msg_send![content_view, addSubview: button];
                pill_buttons.push(SendId(button));
            }

            // Modern list view with icons - Create container for app rows
            let results_padding = 24.0;
            let results_top_margin = 16.0;
            let row_height = 44.0;
            let icon_size = 32.0;

            let results_container_frame = NSRect::new(
                NSPoint::new(results_padding, results_padding),
                NSSize::new(
                    config.window.width as f64 - (results_padding * 2.0),
                    config.window.height as f64 - search_height - pill_height - (results_padding * 3.0) - results_top_margin - 16.0
                ),
            );

            // Create a container view for all rows
            let results_container: id = msg_send![class!(NSView), alloc];
            let results_view: id = msg_send![results_container, initWithFrame: results_container_frame];
            let _: () = msg_send![results_view, setWantsLayer: 1u32];

            // Create rows for first 8 apps with icons
            let workspace_class = class!(NSWorkspace);
            let workspace: id = msg_send![workspace_class, sharedWorkspace];

            // Show 4 random apps initially
            use rand::seq::SliceRandom;
            let mut rng = rand::thread_rng();
            let apps_locked = apps.lock().unwrap();
            let mut app_vec: Vec<_> = apps_locked.iter().collect();
            app_vec.shuffle(&mut rng);
            let initial_apps: Vec<SearchResult> = app_vec.into_iter()
                .take(4)
                .map(|app| SearchResult::new(app.name.clone(), app.path.clone(), SearchMode::Apps))
                .collect();
            drop(apps_locked);

            for (index, result) in initial_apps.iter().enumerate() {
                let y_pos = results_container_frame.size.height - ((index + 1) as f64 * row_height);

                // Create row background view
                let row_frame = NSRect::new(
                    NSPoint::new(0.0, y_pos),
                    NSSize::new(results_container_frame.size.width, row_height),
                );
                let row_view: id = msg_send![class!(NSView), alloc];
                let row_view: id = msg_send![row_view, initWithFrame: row_frame];
                let _: () = msg_send![row_view, setWantsLayer: 1u32];

                // Highlight first row
                if index == 0 {
                    let selection_color = config.get_selection_color();
                    let row_layer: id = msg_send![row_view, layer];
                    let cg_color: id = msg_send![selection_color, CGColor];
                    let _: () = msg_send![row_layer, setBackgroundColor: cg_color];
                    let _: () = msg_send![row_layer, setCornerRadius: 8.0f64];
                }

                // Load icon
                let path_str = NSString::alloc(nil).init_str(&result.path);
                let icon: id = msg_send![workspace, iconForFile: path_str];
                let icon_frame = NSRect::new(
                    NSPoint::new(12.0, (row_height - icon_size) / 2.0),
                    NSSize::new(icon_size, icon_size),
                );
                let icon_view: id = msg_send![class!(NSImageView), alloc];
                let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
                let _: () = msg_send![icon_view, setImage: icon];
                let _: () = msg_send![row_view, addSubview: icon_view];

                // Add app name label
                let label_frame = NSRect::new(
                    NSPoint::new(12.0 + icon_size + 12.0, (row_height - 20.0) / 2.0),
                    NSSize::new(results_container_frame.size.width - icon_size - 36.0, 20.0),
                );
                let label: id = msg_send![class!(NSTextField), alloc];
                let label: id = msg_send![label, initWithFrame: label_frame];
                let _: () = msg_send![label, setEditable: 0u32];
                let _: () = msg_send![label, setSelectable: 0u32];
                let _: () = msg_send![label, setBordered: 0u32];
                let _: () = msg_send![label, setDrawsBackground: 0u32];

                let text_color = if index == 0 {
                    Config::hex_to_nscolor(&config.colors.selection_text)
                } else {
                    config.get_text_color()
                };
                let _: () = msg_send![label, setTextColor: text_color];

                let font_cls = class!(NSFont);
                let font: id = msg_send![font_cls, systemFontOfSize: 15.0f64];
                let _: () = msg_send![label, setFont: font];

                let name_str = NSString::alloc(nil).init_str(&result.name);
                let _: () = msg_send![label, setStringValue: name_str];

                let _: () = msg_send![row_view, addSubview: label];
                let _: () = msg_send![results_view, addSubview: row_view];
            }

            let _: () = msg_send![content_view, addSubview: results_view];

            // Create and configure the text field delegate
            let delegate_class = create_text_field_delegate_class();
            let delegate: id = msg_send![delegate_class, new];

            // Store delegate data in global HashMap
            let delegate_ptr = delegate as usize;
            let mut data_map = DELEGATE_DATA.lock().unwrap();
            if data_map.is_none() {
                *data_map = Some(HashMap::new());
            }

            // Initialize with 4 random apps
            let initial_filtered = Arc::new(Mutex::new(initial_apps.clone()));

            let search_mode = Arc::new(Mutex::new(SearchMode::Apps));

            data_map.as_mut().unwrap().insert(delegate_ptr, DelegateData {
                results_view: SendId(results_view),
                apps: apps.clone(),
                filtered: initial_filtered.clone(),
                selected_index: Arc::new(Mutex::new(0)),
                search_mode: search_mode.clone(),
                search_field: SendId(search_field),
                pill_buttons: pill_buttons.clone(),
            });
            drop(data_map); // Release the lock

            // Set delegate on search field
            let _: () = msg_send![search_field, setDelegate: delegate];

            // Force window to be key and make search field first responder
            let _: () = msg_send![window, makeKeyAndOrderFront: nil];
            let success: bool = msg_send![window, makeFirstResponder: search_field];
            println!("Search field became first responder: {}", success);

            // Also try to explicitly select the text field
            let current_editor: id = msg_send![search_field, currentEditor];
            if current_editor != nil {
                println!("Text field has editor");
            } else {
                println!("Text field has NO editor - trying to activate");
                let _: () = msg_send![search_field, becomeFirstResponder];
            }

            RofiUI {
                _search_field: search_field,
                _results_view: results_view,
                _apps: apps,
                _filtered: initial_filtered,
                _config: config,
                _window: window,
                _pill_buttons: pill_buttons.iter().map(|b| b.0).collect(),
                _search_mode: search_mode,
            }
        }
    }
}
