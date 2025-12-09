use crate::{
    app_search::{fuzzy_search, Application},
    config::Config,
    file_search::{search_files, search_files_random},
    search_mode::{SearchMode, SearchResult},
    system_commands::search_commands,
};
use cocoa::appkit::{NSApp, NSTextField};
use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};

static DELEGATE_CLASS_INIT: Once = Once::new();
static ROW_VIEW_CLASS_INIT: Once = Once::new();

// Wrapper for id that implements Send (safe because all access is on main thread)
#[derive(Clone, Copy)]
struct SendId(id);
unsafe impl Send for SendId {}

// Global storage for delegate data
struct DelegateData {
    results_view: SendId,
    apps: Arc<Mutex<Vec<Application>>>,
    filtered: Arc<Mutex<Vec<SearchResult>>>, // Currently filtered/displayed results
    selected_index: Arc<Mutex<usize>>,       // Currently selected item index
    search_mode: Arc<Mutex<SearchMode>>,     // Current search mode
    _search_field: SendId,                   // Reference to search field for refreshing
    _pill_buttons: Vec<SendId>,              // References to the 3 pill buttons
    config: Config,                          // Configuration for colors and fonts
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
                                app_vec
                                    .into_iter()
                                    .take(4)
                                    .map(|app| {
                                        SearchResult::new(
                                            app.name.clone(),
                                            app.path.clone(),
                                            SearchMode::Apps,
                                        )
                                    })
                                    .collect()
                            } else {
                                fuzzy_search(&data.apps.lock().unwrap(), &query)
                                    .into_iter()
                                    .take(8)
                                    .map(|app| {
                                        SearchResult::new(app.name, app.path, SearchMode::Apps)
                                    })
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
                        SearchMode::Run => search_commands(&query),
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
                    let selection_bg =
                        Config::hex_to_nscolor(&data.config.colors.selection_background);
                    let selection_text = Config::hex_to_nscolor(&data.config.colors.selection_text);
                    let normal_text = Config::hex_to_nscolor(&data.config.colors.text);

                    // Recreate grid cells for filtered results
                    let workspace_class = class!(NSWorkspace);
                    let workspace: id = msg_send![workspace_class, sharedWorkspace];
                    let grid_columns = 5.0;
                    let cell_width = 140.0;
                    let cell_height = 140.0;
                    let icon_size = 88.0;
                    let cell_spacing = 12.0;
                    let frame: NSRect = msg_send![results_view, frame];

                    // Resize results_view to fit all items in grid
                    let num_items = filtered.len();
                    let num_rows = ((num_items as f64) / grid_columns).ceil();
                    let new_height =
                        (num_rows * (cell_height + cell_spacing)).max(frame.size.height);
                    let new_frame = NSRect::new(
                        NSPoint::new(0.0, 0.0),
                        NSSize::new(frame.size.width, new_height),
                    );
                    let _: () = msg_send![results_view, setFrame: new_frame];

                    let container_height = new_height;
                    let selected_idx = *data.selected_index.lock().unwrap();

                    let row_class = create_row_view_class();

                    for (index, result) in filtered.iter().enumerate() {
                        // Calculate grid position
                        let col = (index as f64) % grid_columns;
                        let row = ((index as f64) / grid_columns).floor();

                        let x_pos = col * (cell_width + cell_spacing);
                        let y_pos = container_height - ((row + 1.0) * (cell_height + cell_spacing));

                        // Create cell
                        let cell_frame = NSRect::new(
                            NSPoint::new(x_pos, y_pos),
                            NSSize::new(cell_width, cell_height),
                        );
                        let cell_view: id = msg_send![row_class, alloc];
                        let cell_view: id = msg_send![cell_view, initWithFrame: cell_frame];
                        let _: () = msg_send![cell_view, setWantsLayer: 1u32];

                        (*cell_view).set_ivar("rowIndex", index as isize);

                        let cell_layer: id = msg_send![cell_view, layer];
                        let _: () = msg_send![cell_layer, setCornerRadius: 10.0f64];
                        if index == selected_idx {
                            let cg_color: id = msg_send![selection_bg, CGColor];
                            let _: () = msg_send![cell_layer, setBackgroundColor: cg_color];
                        }

                        // Icon centered at top
                        if result.result_type == SearchMode::Apps
                            || result.result_type == SearchMode::Files
                        {
                            let path_str = NSString::alloc(nil).init_str(&result.path);
                            let icon: id = msg_send![workspace, iconForFile: path_str];
                            let icon_ns_size = NSSize::new(icon_size, icon_size);
                            let _: () = msg_send![icon, setSize: icon_ns_size];
                            let icon_x = (cell_width - icon_size) / 2.0;
                            let icon_y = cell_height - icon_size - 16.0;
                            let icon_frame = NSRect::new(
                                NSPoint::new(icon_x, icon_y),
                                NSSize::new(icon_size, icon_size),
                            );
                            let icon_view: id = msg_send![class!(NSImageView), alloc];
                            let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
                            let _: () = msg_send![icon_view, setImage: icon];
                            let _: () = msg_send![icon_view, setImageScaling: 3i64];
                            let _: () = msg_send![cell_view, addSubview: icon_view];
                        }

                        // Label centered below
                        let label_frame = NSRect::new(
                            NSPoint::new(4.0, 8.0),
                            NSSize::new(cell_width - 8.0, 28.0),
                        );
                        let label: id = msg_send![class!(NSTextField), alloc];
                        let label: id = msg_send![label, initWithFrame: label_frame];
                        let _: () = msg_send![label, setEditable: 0u32];
                        let _: () = msg_send![label, setSelectable: 0u32];
                        let _: () = msg_send![label, setBordered: 0u32];
                        let _: () = msg_send![label, setDrawsBackground: 0u32];
                        let _: () = msg_send![label, setAlignment: 1i64];
                        let text_color = if index == selected_idx {
                            selection_text
                        } else {
                            normal_text
                        };
                        let _: () = msg_send![label, setTextColor: text_color];
                        let font_cls = class!(NSFont);
                        let font: id = msg_send![font_cls, systemFontOfSize: 14.0f64];
                        let _: () = msg_send![label, setFont: font];
                        let name_str = NSString::alloc(nil).init_str(&result.name);
                        let _: () = msg_send![label, setStringValue: name_str];
                        let _: () = msg_send![label, setLineBreakMode: 4i64];

                        let _: () = msg_send![cell_view, addSubview: label];
                        let _: () = msg_send![results_view, addSubview: cell_view];
                    }

                    // Scroll to top after filtering
                    let scroll_view: id = msg_send![results_view, enclosingScrollView];
                    if scroll_view != nil {
                        let clip_view: id = msg_send![scroll_view, contentView];
                        let clip_bounds: NSRect = msg_send![clip_view, bounds];
                        let doc_frame: NSRect = msg_send![results_view, frame];

                        // Scroll to show the top of the document (highest y values)
                        let scroll_point = NSPoint::new(
                            0.0,
                            (doc_frame.size.height - clip_bounds.size.height).max(0.0),
                        );
                        let _: () = msg_send![results_view, scrollPoint: scroll_point];
                    }
                }
            }

            // Handle command keys (Escape, Enter)
            extern "C" fn control_text_view_do_command_by_selector(
                _this: &Object,
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

                    // Debug: print all selectors received
                    println!("Selector received: {}", sel_str);

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
                                println!(
                                    "Launching: {} (type: {:?})",
                                    result.name, result.result_type
                                );

                                match result.result_type {
                                    SearchMode::Apps | SearchMode::Files => {
                                        // Launch application or open file using NSWorkspace
                                        let workspace_class = class!(NSWorkspace);
                                        let workspace: id =
                                            msg_send![workspace_class, sharedWorkspace];
                                        let path_string =
                                            NSString::alloc(nil).init_str(&result.path);

                                        // Use launchApplication for apps, openFile for other files
                                        if result.result_type == SearchMode::Apps {
                                            let _: bool = msg_send![workspace, launchApplication: path_string];
                                        } else {
                                            let url_class = class!(NSURL);
                                            let url: id = msg_send![url_class, fileURLWithPath: path_string];
                                            let _: bool = msg_send![workspace, openURL: url];
                                        }
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

                    // Arrow Down triggers "moveDown:" - move to next row (5 items)
                    if sel_str == "moveDown:" {
                        println!("Arrow Down pressed");
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let grid_cols: usize = 5;
                            let filtered_count = data.filtered.lock().unwrap().len();
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            let new_idx = *selected_idx + grid_cols;
                            if new_idx < filtered_count {
                                *selected_idx = new_idx;
                                println!("Selection moved to: {}", *selected_idx);
                            }
                            drop(selected_idx);

                            // Manually rebuild UI
                            let results_view = data.results_view.0;
                            let filtered = data.filtered.lock().unwrap().clone();
                            let selected_index = *data.selected_index.lock().unwrap();
                            let config = data.config.clone();
                            drop(data_map);

                            // Clear subviews
                            loop {
                                let subviews: id = msg_send![results_view, subviews];
                                let count: usize = msg_send![subviews, count];
                                if count == 0 {
                                    break;
                                }
                                let subview: id = msg_send![subviews, firstObject];
                                let _: () = msg_send![subview, removeFromSuperview];
                            }

                            // Rebuild rows
                            let row_class = create_row_view_class();
                            let selection_bg =
                                Config::hex_to_nscolor(&config.colors.selection_background);
                            let selection_text =
                                Config::hex_to_nscolor(&config.colors.selection_text);
                            let normal_text = Config::hex_to_nscolor(&config.colors.text);
                            let workspace_class = class!(NSWorkspace);
                            let workspace: id = msg_send![workspace_class, sharedWorkspace];
                            let grid_columns = 5.0;
                            let cell_width = 140.0;
                            let cell_height = 140.0;
                            let icon_size = 88.0;
                            let cell_spacing = 12.0;
                            let frame: NSRect = msg_send![results_view, frame];

                            // Resize results_view to fit all items in grid
                            let num_items = filtered.len();
                            let num_rows = ((num_items as f64) / grid_columns).ceil();
                            let new_height =
                                (num_rows * (cell_height + cell_spacing)).max(frame.size.height);
                            let new_frame = NSRect::new(
                                NSPoint::new(0.0, 0.0),
                                NSSize::new(frame.size.width, new_height),
                            );
                            let _: () = msg_send![results_view, setFrame: new_frame];

                            let container_height = new_height;

                            for (index, result) in filtered.iter().enumerate() {
                                let col = (index as f64) % grid_columns;
                                let row = ((index as f64) / grid_columns).floor();
                                let x_pos = col * (cell_width + cell_spacing);
                                let y_pos =
                                    container_height - ((row + 1.0) * (cell_height + cell_spacing));
                                let cell_frame = NSRect::new(
                                    NSPoint::new(x_pos, y_pos),
                                    NSSize::new(cell_width, cell_height),
                                );
                                let cell_view: id = msg_send![row_class, alloc];
                                let cell_view: id = msg_send![cell_view, initWithFrame: cell_frame];
                                let _: () = msg_send![cell_view, setWantsLayer: 1u32];

                                (*cell_view).set_ivar("rowIndex", index as isize);

                                let cell_layer: id = msg_send![cell_view, layer];
                                let _: () = msg_send![cell_layer, setCornerRadius: 10.0f64];
                                if index == selected_index {
                                    let cg_color: id = msg_send![selection_bg, CGColor];
                                    let _: () = msg_send![cell_layer, setBackgroundColor: cg_color];
                                }

                                if result.result_type == SearchMode::Apps
                                    || result.result_type == SearchMode::Files
                                {
                                    let path_str = NSString::alloc(nil).init_str(&result.path);
                                    let icon: id = msg_send![workspace, iconForFile: path_str];
                                    let icon_ns_size = NSSize::new(icon_size, icon_size);
                                    let _: () = msg_send![icon, setSize: icon_ns_size];
                                    let icon_x = (cell_width - icon_size) / 2.0;
                                    let icon_y = cell_height - icon_size - 16.0;
                                    let icon_frame = NSRect::new(
                                        NSPoint::new(icon_x, icon_y),
                                        NSSize::new(icon_size, icon_size),
                                    );
                                    let icon_view: id = msg_send![class!(NSImageView), alloc];
                                    let icon_view: id =
                                        msg_send![icon_view, initWithFrame: icon_frame];
                                    let _: () = msg_send![icon_view, setImage: icon];
                                    let _: () = msg_send![icon_view, setImageScaling: 3i64];
                                    let _: () = msg_send![cell_view, addSubview: icon_view];
                                }

                                let label_frame = NSRect::new(
                                    NSPoint::new(4.0, 8.0),
                                    NSSize::new(cell_width - 8.0, 28.0),
                                );
                                let label: id = msg_send![class!(NSTextField), alloc];
                                let label: id = msg_send![label, initWithFrame: label_frame];
                                let _: () = msg_send![label, setEditable: 0u32];
                                let _: () = msg_send![label, setSelectable: 0u32];
                                let _: () = msg_send![label, setBordered: 0u32];
                                let _: () = msg_send![label, setDrawsBackground: 0u32];
                                let _: () = msg_send![label, setAlignment: 1i64];
                                let text_color = if index == selected_index {
                                    selection_text
                                } else {
                                    normal_text
                                };
                                let _: () = msg_send![label, setTextColor: text_color];
                                let font_cls = class!(NSFont);
                                let font: id = msg_send![font_cls, systemFontOfSize: 14.0f64];
                                let _: () = msg_send![label, setFont: font];
                                let name_str = NSString::alloc(nil).init_str(&result.name);
                                let _: () = msg_send![label, setStringValue: name_str];
                                let _: () = msg_send![label, setLineBreakMode: 4i64];
                                let _: () = msg_send![cell_view, addSubview: label];
                                let _: () = msg_send![results_view, addSubview: cell_view];
                            }
                        }
                        return YES as u8;
                    }

                    // Arrow Up triggers "moveUp:" - move to previous row (5 items)
                    if sel_str == "moveUp:" {
                        println!("Arrow Up pressed");
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let grid_cols: usize = 5;
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx >= grid_cols {
                                *selected_idx -= grid_cols;
                                println!("Selection moved to: {}", *selected_idx);
                            }
                            drop(selected_idx);

                            // Manually rebuild UI
                            let results_view = data.results_view.0;
                            let filtered = data.filtered.lock().unwrap().clone();
                            let selected_index = *data.selected_index.lock().unwrap();
                            let config = data.config.clone();
                            drop(data_map);

                            // Clear subviews
                            loop {
                                let subviews: id = msg_send![results_view, subviews];
                                let count: usize = msg_send![subviews, count];
                                if count == 0 {
                                    break;
                                }
                                let subview: id = msg_send![subviews, firstObject];
                                let _: () = msg_send![subview, removeFromSuperview];
                            }

                            // Rebuild rows
                            let row_class = create_row_view_class();
                            let selection_bg =
                                Config::hex_to_nscolor(&config.colors.selection_background);
                            let selection_text =
                                Config::hex_to_nscolor(&config.colors.selection_text);
                            let normal_text = Config::hex_to_nscolor(&config.colors.text);
                            let workspace_class = class!(NSWorkspace);
                            let workspace: id = msg_send![workspace_class, sharedWorkspace];
                            let grid_columns = 5.0;
                            let cell_width = 140.0;
                            let cell_height = 140.0;
                            let icon_size = 88.0;
                            let cell_spacing = 12.0;
                            let frame: NSRect = msg_send![results_view, frame];

                            // Resize results_view to fit all items in grid
                            let num_items = filtered.len();
                            let num_rows = ((num_items as f64) / grid_columns).ceil();
                            let new_height =
                                (num_rows * (cell_height + cell_spacing)).max(frame.size.height);
                            let new_frame = NSRect::new(
                                NSPoint::new(0.0, 0.0),
                                NSSize::new(frame.size.width, new_height),
                            );
                            let _: () = msg_send![results_view, setFrame: new_frame];

                            let container_height = new_height;

                            for (index, result) in filtered.iter().enumerate() {
                                let col = (index as f64) % grid_columns;
                                let row = ((index as f64) / grid_columns).floor();
                                let x_pos = col * (cell_width + cell_spacing);
                                let y_pos =
                                    container_height - ((row + 1.0) * (cell_height + cell_spacing));
                                let cell_frame = NSRect::new(
                                    NSPoint::new(x_pos, y_pos),
                                    NSSize::new(cell_width, cell_height),
                                );
                                let cell_view: id = msg_send![row_class, alloc];
                                let cell_view: id = msg_send![cell_view, initWithFrame: cell_frame];
                                let _: () = msg_send![cell_view, setWantsLayer: 1u32];

                                (*cell_view).set_ivar("rowIndex", index as isize);

                                let cell_layer: id = msg_send![cell_view, layer];
                                let _: () = msg_send![cell_layer, setCornerRadius: 10.0f64];
                                if index == selected_index {
                                    let cg_color: id = msg_send![selection_bg, CGColor];
                                    let _: () = msg_send![cell_layer, setBackgroundColor: cg_color];
                                }

                                if result.result_type == SearchMode::Apps
                                    || result.result_type == SearchMode::Files
                                {
                                    let path_str = NSString::alloc(nil).init_str(&result.path);
                                    let icon: id = msg_send![workspace, iconForFile: path_str];
                                    let icon_ns_size = NSSize::new(icon_size, icon_size);
                                    let _: () = msg_send![icon, setSize: icon_ns_size];
                                    let icon_x = (cell_width - icon_size) / 2.0;
                                    let icon_y = cell_height - icon_size - 16.0;
                                    let icon_frame = NSRect::new(
                                        NSPoint::new(icon_x, icon_y),
                                        NSSize::new(icon_size, icon_size),
                                    );
                                    let icon_view: id = msg_send![class!(NSImageView), alloc];
                                    let icon_view: id =
                                        msg_send![icon_view, initWithFrame: icon_frame];
                                    let _: () = msg_send![icon_view, setImage: icon];
                                    let _: () = msg_send![icon_view, setImageScaling: 3i64];
                                    let _: () = msg_send![cell_view, addSubview: icon_view];
                                }

                                let label_frame = NSRect::new(
                                    NSPoint::new(4.0, 8.0),
                                    NSSize::new(cell_width - 8.0, 28.0),
                                );
                                let label: id = msg_send![class!(NSTextField), alloc];
                                let label: id = msg_send![label, initWithFrame: label_frame];
                                let _: () = msg_send![label, setEditable: 0u32];
                                let _: () = msg_send![label, setSelectable: 0u32];
                                let _: () = msg_send![label, setBordered: 0u32];
                                let _: () = msg_send![label, setDrawsBackground: 0u32];
                                let _: () = msg_send![label, setAlignment: 1i64];
                                let text_color = if index == selected_index {
                                    selection_text
                                } else {
                                    normal_text
                                };
                                let _: () = msg_send![label, setTextColor: text_color];
                                let font_cls = class!(NSFont);
                                let font: id = msg_send![font_cls, systemFontOfSize: 14.0f64];
                                let _: () = msg_send![label, setFont: font];
                                let name_str = NSString::alloc(nil).init_str(&result.name);
                                let _: () = msg_send![label, setStringValue: name_str];
                                let _: () = msg_send![label, setLineBreakMode: 4i64];
                                let _: () = msg_send![cell_view, addSubview: label];
                                let _: () = msg_send![results_view, addSubview: cell_view];
                            }
                        }
                        return YES as u8;
                    }

                    // Arrow Right triggers "moveRight:" - move to next item
                    if sel_str == "moveRight:" {
                        println!("Arrow Right pressed");
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let filtered_count = data.filtered.lock().unwrap().len();
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx < filtered_count.saturating_sub(1) {
                                *selected_idx += 1;
                                println!("Selection moved to: {}", *selected_idx);
                            }
                            let new_selected = *selected_idx;
                            drop(selected_idx);

                            // Update cell backgrounds for visual selection
                            let results_view = data.results_view.0;
                            let selection_bg =
                                Config::hex_to_nscolor(&data.config.colors.selection_background);
                            let clear_color: id = msg_send![class!(NSColor), clearColor];

                            let subviews: id = msg_send![results_view, subviews];
                            let count: usize = msg_send![subviews, count];
                            for i in 0..count {
                                let cell_view: id = msg_send![subviews, objectAtIndex: i];
                                let layer: id = msg_send![cell_view, layer];
                                if layer != nil {
                                    // Get the row index from the cell
                                    let row_idx: isize =
                                        *(&*cell_view as &Object).get_ivar::<isize>("rowIndex");
                                    if row_idx == new_selected as isize {
                                        let cg_color: id = msg_send![selection_bg, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    } else {
                                        let cg_color: id = msg_send![clear_color, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    }
                                }
                            }
                        }
                        return YES as u8;
                    }

                    // Arrow Left triggers "moveLeft:" - move to previous item
                    if sel_str == "moveLeft:" {
                        println!("Arrow Left pressed");
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx > 0 {
                                *selected_idx -= 1;
                                println!("Selection moved to: {}", *selected_idx);
                            }
                            let new_selected = *selected_idx;
                            drop(selected_idx);

                            // Update cell backgrounds for visual selection
                            let results_view = data.results_view.0;
                            let selection_bg =
                                Config::hex_to_nscolor(&data.config.colors.selection_background);
                            let clear_color: id = msg_send![class!(NSColor), clearColor];

                            let subviews: id = msg_send![results_view, subviews];
                            let count: usize = msg_send![subviews, count];
                            for i in 0..count {
                                let cell_view: id = msg_send![subviews, objectAtIndex: i];
                                let layer: id = msg_send![cell_view, layer];
                                if layer != nil {
                                    // Get the row index from the cell
                                    let row_idx: isize =
                                        *(&*cell_view as &Object).get_ivar::<isize>("rowIndex");
                                    if row_idx == new_selected as isize {
                                        let cg_color: id = msg_send![selection_bg, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    } else {
                                        let cg_color: id = msg_send![clear_color, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    }
                                }
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
                    control_text_view_do_command_by_selector
                        as extern "C" fn(&Object, Sel, id, id, Sel) -> u8,
                );
            }

            decl.register();
        });

        Class::get("RofiTextFieldDelegate").unwrap()
    }
}

// Create a custom row view class that handles hover and click
fn create_row_view_class() -> *const Class {
    unsafe {
        ROW_VIEW_CLASS_INIT.call_once(|| {
            let superclass = class!(NSView);
            let mut decl = ClassDecl::new("ClickableRowView", superclass).unwrap();

            // Store row index as an ivar
            decl.add_ivar::<isize>("rowIndex");

            // Mouse entered - highlight the row with hover effect
            extern "C" fn mouse_entered(this: &mut Object, _: Sel, _event: id) {
                unsafe {
                    let row_index: isize = *this.get_ivar("rowIndex");
                    println!("Mouse entered row: {}", row_index);


                    // Apply hover background color directly to this cell
let layer: id = msg_send![this, layer];
if layer != nil {
    // Use selection background color from config (matches arrow key selection)
    let hover_color = Config::hex_to_nscolor("#d79921");
    let hover_cg: id = msg_send![hover_color, CGColor];
    let _: () = msg_send![layer, setBackgroundColor: hover_cg];
}

                    // Also update the selected index
                    let window: id = msg_send![this, window];
                    if window == nil {
                        return;
                    }

                    let content_view: id = msg_send![window, contentView];
                    let subviews: id = msg_send![content_view, subviews];
                    let count: usize = msg_send![subviews, count];

                    // Find the text field
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
                            // Update selected index to this row
                            *data.selected_index.lock().unwrap() = row_index as usize;
                        }
                    }
                }
            }

            // Mouse exited - remove hover highlight
            extern "C" fn mouse_exited(this: &mut Object, _: Sel, _event: id) {
                unsafe {
                    println!("Mouse exited row");
                    // Remove hover background color
                    let layer: id = msg_send![this, layer];
                    if layer != nil {
                        // Clear background (transparent)
                        let clear_color: id = msg_send![class!(NSColor), clearColor];
                        let clear_cg: id = msg_send![clear_color, CGColor];
                        let _: () = msg_send![layer, setBackgroundColor: clear_cg];
                    }
                }
            }

// Mouse down - launch the app
extern "C" fn mouse_down(this: &mut Object, _: Sel, _event: id) {
    unsafe {
        let row_index: isize = *this.get_ivar("rowIndex");
        println!("Mouse clicked row: {}", row_index);

        // Get delegate data and launch the selected item
        let window: id = msg_send![this, window];
        if window == nil {
            println!("Window is nil");
            return;
        }

        let content_view: id = msg_send![window, contentView];

        // Search recursively for NSTextField (it's inside search_container)
        fn find_text_field(view: id) -> id {
            unsafe {
                let subviews: id = msg_send![view, subviews];
                let count: usize = msg_send![subviews, count];
                for i in 0..count {
                    let subview: id = msg_send![subviews, objectAtIndex: i];
                    let class_name: id = msg_send![subview, className];
                    let cstr: *const i8 = msg_send![class_name, UTF8String];
                    let name = std::ffi::CStr::from_ptr(cstr).to_string_lossy();
                    if name == "NSTextField" {
                        // Check if it's editable (the search field, not a label)
                        let editable: bool = msg_send![subview, isEditable];
                        if editable {
                            return subview;
                        }
                    }
                    // Recurse into subviews
                    let found = find_text_field(subview);
                    if found != nil {
                        return found;
                    }
                }
                nil
            }
        }

        let text_field = find_text_field(content_view);

        if text_field == nil {
            println!("Text field not found!");
            return;
        }

        println!("Found text field");

        let delegate: id = msg_send![text_field, delegate];
        let delegate_ptr = delegate as usize;

        let data_map = DELEGATE_DATA.lock().unwrap();
        if let Some(data) = data_map.as_ref().and_then(|m| m.get(&delegate_ptr)) {
            let filtered = data.filtered.lock().unwrap();
            if let Some(result) = filtered.get(row_index as usize) {
                println!("Launching: {} (type: {:?})", result.name, result.result_type);

                match result.result_type {
                    SearchMode::Apps | SearchMode::Files => {
                        let workspace_class = class!(NSWorkspace);
                        let workspace: id = msg_send![workspace_class, sharedWorkspace];

                        let path_string = NSString::alloc(nil).init_str(&result.path);
                        let url: id = msg_send![class!(NSURL), fileURLWithPath: path_string];

                        let success: bool = msg_send![workspace, openURL: url];
                        println!("openURL success: {}", success);
                    }
                    SearchMode::Run => {
                        std::process::Command::new("sh")
                            .arg("-c")
                            .arg(&result.path)
                            .spawn()
                            .ok();
                    }
                }

                // Close window after launching
                let app = NSApp();
                let _: () = msg_send![app, terminate: nil];
            } else {
                println!("No result at index {}", row_index);
            }
        } else {
            println!("Delegate data not found");
        }
    }
}

            // Update tracking areas to receive mouse events
            extern "C" fn update_tracking_areas(this: &mut Object, _: Sel) {
                unsafe {
                    // No need to call super for this simple case

                    // Remove old tracking areas
                    let tracking_areas: id = msg_send![this, trackingAreas];
                    let count: usize = msg_send![tracking_areas, count];
                    for i in 0..count {
                        let area: id = msg_send![tracking_areas, objectAtIndex: i];
                        let _: () = msg_send![this, removeTrackingArea: area];
                    }

                    // Add new tracking area
                    let bounds: NSRect = msg_send![this, bounds];
                    // NSTrackingMouseEnteredAndExited = 0x01
                    // NSTrackingActiveAlways = 0x80
                    // NSTrackingInVisibleRect = 0x200
                    let options: usize = 0x01 | 0x80 | 0x200;
                    let tracking_area: id = msg_send![class!(NSTrackingArea), alloc];
                    let this_ptr = this as *mut Object as id;
                    let tracking_area: id = msg_send![tracking_area, initWithRect:bounds options:options owner:this_ptr userInfo:nil];
                    let _: () = msg_send![this, addTrackingArea: tracking_area];
                }
            }

            unsafe {
                decl.add_method(
                    sel!(mouseEntered:),
                    mouse_entered as extern "C" fn(&mut Object, Sel, id),
                );
                decl.add_method(
                    sel!(mouseExited:),
                    mouse_exited as extern "C" fn(&mut Object, Sel, id),
                );
                decl.add_method(
                    sel!(mouseDown:),
                    mouse_down as extern "C" fn(&mut Object, Sel, id),
                );
                decl.add_method(
                    sel!(updateTrackingAreas),
                    update_tracking_areas as extern "C" fn(&mut Object, Sel),
                );
            }

            decl.register();
        });

        Class::get("ClickableRowView").unwrap()
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

            // Get actual window dimensions
            let window_frame: NSRect = msg_send![window, frame];
            let window_width = window_frame.size.width;
            let window_height = window_frame.size.height;

            // Modern UI: Create search container with icon
            let search_padding = 0.0; // Full width
            let search_height = 70.0; // Taller for larger text
            let search_container_frame = NSRect::new(
                NSPoint::new(
                    search_padding,
                    window_height - search_height - search_padding,
                ),
                NSSize::new(window_width - (search_padding * 2.0), search_height),
            );

            // Create search container view
            let search_container: id = msg_send![class!(NSView), alloc];
            let search_container: id =
                msg_send![search_container, initWithFrame: search_container_frame];
            let _: () = msg_send![search_container, setWantsLayer: 1u32];

            // Background for search container
            let input_bg_color = Config::hex_to_nscolor(&config.colors.input_background);
            let _: () = msg_send![search_container, setBackgroundColor: input_bg_color];
            let container_layer: id = msg_send![search_container, layer];
            let _: () = msg_send![container_layer, setCornerRadius: 0.0f64]; // No rounding for full width

            // Add search icon using SF Symbols (magnifyingglass)
            let icon_size = 24.0;
            let icon_x = 20.0;
            let icon_y = (search_height - icon_size) / 2.0;
            let icon_frame = NSRect::new(
                NSPoint::new(icon_x, icon_y),
                NSSize::new(icon_size, icon_size),
            );

            // Create NSImage from SF Symbol with specific point size
            let symbol_name = NSString::alloc(nil).init_str("magnifyingglass");
            let symbol_config: id = msg_send![class!(NSImageSymbolConfiguration), configurationWithPointSize:28.0f64 weight:1i64];
            let symbol_image: id = msg_send![class!(NSImage), imageWithSystemSymbolName:symbol_name accessibilityDescription:nil];
            let sized_image: id =
                msg_send![symbol_image, imageWithSymbolConfiguration:symbol_config];

            // Create image view
            let icon_view: id = msg_send![class!(NSImageView), alloc];
            let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
            let _: () = msg_send![icon_view, setImage: sized_image];
            let _: () =
                msg_send![icon_view, setContentTintColor: Config::hex_to_nscolor("#ffffff")];
            let _: () = msg_send![search_container, addSubview: icon_view];

            // Create text field starting after icon with proper spacing
            let text_field_x = icon_x + icon_size + 10.0;
            let text_field_width = window_width - text_field_x - 20.0;

            let text_field_height = 40.0;
            let text_field_y = (search_height - text_field_height) / 2.0 - 6.0; // Adjust baseline down

            let search_frame = NSRect::new(
                NSPoint::new(text_field_x, text_field_y),
                NSSize::new(text_field_width, text_field_height),
            );

            let search_field_alloc = NSTextField::alloc(nil);
            let search_field: id = msg_send![search_field_alloc, initWithFrame: search_frame];

            // Create placeholder
            let placeholder_text = NSString::alloc(nil).init_str("Search");
            let placeholder_color = Config::hex_to_nscolor("#ffffff");
            let attrs_dict: id = msg_send![class!(NSMutableDictionary), new];
            let foreground_key = NSString::alloc(nil).init_str("NSColor");
            let _: () = msg_send![attrs_dict, setObject:placeholder_color forKey:foreground_key];
            // Add font to placeholder attributes
            let font_key = NSString::alloc(nil).init_str("NSFont");
            let placeholder_font: id = msg_send![class!(NSFont), systemFontOfSize: 20.0f64];
            let _: () = msg_send![attrs_dict, setObject:placeholder_font forKey:font_key];
            // Add baseline offset to push placeholder down (negative = down)
            let baseline_key = NSString::alloc(nil).init_str("NSBaselineOffset");
            let baseline_offset: id = msg_send![class!(NSNumber), numberWithFloat: -8.0f32];
            let _: () = msg_send![attrs_dict, setObject:baseline_offset forKey:baseline_key];
            let placeholder_attr: id = msg_send![class!(NSAttributedString), alloc];
            let placeholder_attr: id =
                msg_send![placeholder_attr, initWithString:placeholder_text attributes:attrs_dict];
            let _: () = msg_send![search_field, setPlaceholderAttributedString: placeholder_attr];

            let _: () = msg_send![search_field, setBezeled: 0u32];
            let _: () = msg_send![search_field, setBordered: 0u32];
            let _: () = msg_send![search_field, setEditable: 1u32];
            let _: () = msg_send![search_field, setSelectable: 1u32];
            let _: () = msg_send![search_field, setDrawsBackground: 0u32]; // Transparent
            let _: () = msg_send![search_field, setFocusRingType: 1u32]; // NSFocusRingTypeNone = 1

            // White text on tan background
            let text_color = Config::hex_to_nscolor("#ffffff");
            let _: () = msg_send![search_field, setTextColor: text_color];

            // Set font for search field
            let font_cls = class!(NSFont);
            let font_name = NSString::alloc(nil).init_str(&config.font.family);
            let font_size = 20.0f64;
            let font: id = msg_send![font_cls, fontWithName:font_name size:font_size];
            let font = if font == nil {
                msg_send![font_cls, systemFontOfSize: font_size]
            } else {
                font
            };
            let _: () = msg_send![search_field, setFont: font];

            // Configure cell for single-line input
            let _: () = msg_send![search_field, setAlignment: 0i64];
            let cell: id = msg_send![search_field, cell];
            let _: () = msg_send![cell, setUsesSingleLineMode: 1u32];
            let _: () = msg_send![cell, setScrollable: 1u32];
            let _: () = msg_send![cell, setLineBreakMode: 4i64];
            let _: () = msg_send![cell, setFocusRingType: 1u32]; // NSFocusRingTypeNone = 1
            let _: () = msg_send![search_field, setRefusesFirstResponder: 0u32];

            let _: () = msg_send![search_container, addSubview: search_field];

            // Add search container to window
            let content_view: id = msg_send![window, contentView];
            let _: () = msg_send![content_view, addSubview: search_container];

            // Pill buttons removed for cleaner UI matching reference design
            let pill_height = 0.0; // No pill buttons
            let pill_buttons: Vec<SendId> = Vec::new();

            // Modern grid view with icons - Create container for app cells
            let results_padding = 24.0;
            let results_top_margin = 8.0;
            let grid_columns = 5.0; // 5 apps per row
            let cell_width = 140.0; // Width of each grid cell
            let cell_height = 140.0; // Height of each grid cell (icon + label)
            let icon_size = 88.0; // Larger icons for grid view
            let cell_spacing = 12.0; // Spacing between cells

            let results_container_frame = NSRect::new(
                NSPoint::new(results_padding, results_padding),
                NSSize::new(
                    window_width - (results_padding * 2.0),
                    window_height
                        - search_height
                        - pill_height
                        - results_padding
                        - results_top_margin
                        - 32.0,
                ),
            );

            // Create a scroll view for results
            let scroll_view: id = msg_send![class!(NSScrollView), alloc];
            let scroll_view: id = msg_send![scroll_view, initWithFrame: results_container_frame];
            let _: () = msg_send![scroll_view, setHasVerticalScroller: 0u32]; // Hide scrollbar
            let _: () = msg_send![scroll_view, setHasHorizontalScroller: 0u32];
            let _: () = msg_send![scroll_view, setBorderType: 0i64]; // NSNoBorder
            let _: () = msg_send![scroll_view, setDrawsBackground: 0u32];
            let _: () = msg_send![scroll_view, setAutohidesScrollers: 1u32];

            // Create a container view for all rows (document view of scroll view)
            let results_container: id = msg_send![class!(NSView), alloc];
            let results_view: id = msg_send![results_container, initWithFrame: NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(results_container_frame.size.width, 1000.0) // Large enough for many items
            )];
            let _: () = msg_send![results_view, setWantsLayer: 1u32];

            // Set the results view as the document view of the scroll view
            let _: () = msg_send![scroll_view, setDocumentView: results_view];

            // Create rows for first 8 apps with icons
            let workspace_class = class!(NSWorkspace);
            let workspace: id = msg_send![workspace_class, sharedWorkspace];

            // Show 15 random apps initially (3 rows x 5 columns)
            use rand::seq::SliceRandom;
            let mut rng = rand::thread_rng();
            let apps_locked = apps.lock().unwrap();
            let mut app_vec: Vec<_> = apps_locked.iter().collect();
            app_vec.shuffle(&mut rng);
            let initial_apps: Vec<SearchResult> = app_vec
                .into_iter()
                .take(15)
                .map(|app| SearchResult::new(app.name.clone(), app.path.clone(), SearchMode::Apps))
                .collect();
            drop(apps_locked);

            let row_class = create_row_view_class();

            for (index, result) in initial_apps.iter().enumerate() {
                // Calculate grid position (column, row)
                let col = (index as f64) % grid_columns;
                let row = ((index as f64) / grid_columns).floor();

                // Calculate x, y position for this cell
                let x_pos = col * (cell_width + cell_spacing);
                let y_pos = results_container_frame.size.height
                    - ((row + 1.0) * (cell_height + cell_spacing));

                // Create cell background view
                let cell_frame = NSRect::new(
                    NSPoint::new(x_pos, y_pos),
                    NSSize::new(cell_width, cell_height),
                );
                let cell_view: id = msg_send![row_class, alloc];
                let cell_view: id = msg_send![cell_view, initWithFrame: cell_frame];
                let _: () = msg_send![cell_view, setWantsLayer: 1u32];

                // Set row index for click/hover handling
                (*cell_view).set_ivar("rowIndex", index as isize);

                // Highlight first cell
                let cell_layer: id = msg_send![cell_view, layer];
                let _: () = msg_send![cell_layer, setCornerRadius: 10.0f64];
                if index == 0 {
                    let selection_color = config.get_selection_color();
                    let cg_color: id = msg_send![selection_color, CGColor];
                    let _: () = msg_send![cell_layer, setBackgroundColor: cg_color];
                }

                // Load icon - centered at top of cell
                let path_str = NSString::alloc(nil).init_str(&result.path);
                let icon: id = msg_send![workspace, iconForFile: path_str];
                // Set the icon size to match our desired size
                let icon_ns_size = NSSize::new(icon_size, icon_size);
                let _: () = msg_send![icon, setSize: icon_ns_size];

                let icon_x = (cell_width - icon_size) / 2.0;
                let icon_y = cell_height - icon_size - 16.0; // 16px from top
                let icon_frame = NSRect::new(
                    NSPoint::new(icon_x, icon_y),
                    NSSize::new(icon_size, icon_size),
                );
                let icon_view: id = msg_send![class!(NSImageView), alloc];
                let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
                let _: () = msg_send![icon_view, setImage: icon];
                // NSImageScaleProportionallyUpOrDown = 3
                let _: () = msg_send![icon_view, setImageScaling: 3i64];
                let _: () = msg_send![cell_view, addSubview: icon_view];

                // Add app name label - centered below icon
                let label_height = 28.0;
                let label_y = 8.0; // 8px from bottom
                let label_frame = NSRect::new(
                    NSPoint::new(4.0, label_y),
                    NSSize::new(cell_width - 8.0, label_height),
                );
                let label: id = msg_send![class!(NSTextField), alloc];
                let label: id = msg_send![label, initWithFrame: label_frame];
                let _: () = msg_send![label, setEditable: 0u32];
                let _: () = msg_send![label, setSelectable: 0u32];
                let _: () = msg_send![label, setBordered: 0u32];
                let _: () = msg_send![label, setDrawsBackground: 0u32];
                let _: () = msg_send![label, setAlignment: 1i64]; // NSTextAlignmentCenter

                let text_color = if index == 0 {
                    Config::hex_to_nscolor(&config.colors.selection_text)
                } else {
                    config.get_text_color()
                };
                let _: () = msg_send![label, setTextColor: text_color];

                let font_cls = class!(NSFont);
                let font: id = msg_send![font_cls, systemFontOfSize: 14.0f64];
                let _: () = msg_send![label, setFont: font];

                // Truncate long names
                let name_str = NSString::alloc(nil).init_str(&result.name);
                let _: () = msg_send![label, setStringValue: name_str];
                let _: () = msg_send![label, setLineBreakMode: 4i64]; // Truncate tail

                let _: () = msg_send![cell_view, addSubview: label];
                let _: () = msg_send![results_view, addSubview: cell_view];
            }

            let _: () = msg_send![content_view, addSubview: scroll_view];

            // Scroll to top to show first row of apps
            let clip_view: id = msg_send![scroll_view, contentView];
            let doc_view: id = msg_send![scroll_view, documentView];
            let doc_frame: NSRect = msg_send![doc_view, frame];
            let scroll_point = NSPoint::new(0.0, doc_frame.size.height);
            let _: () = msg_send![clip_view, scrollToPoint: scroll_point];
            let _: () = msg_send![scroll_view, reflectScrolledClipView: clip_view];

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

            data_map.as_mut().unwrap().insert(
                delegate_ptr,
                DelegateData {
                    results_view: SendId(results_view),
                    apps: apps.clone(),
                    filtered: initial_filtered.clone(),
                    selected_index: Arc::new(Mutex::new(0)),
                    search_mode: search_mode.clone(),
                    _search_field: SendId(search_field),
                    _pill_buttons: pill_buttons.clone(),
                    config: config.clone(),
                },
            );
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
