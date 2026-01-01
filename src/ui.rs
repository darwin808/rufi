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
use core_graphics::geometry::CGSize;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};

static DELEGATE_CLASS_INIT: Once = Once::new();
static ROW_VIEW_CLASS_INIT: Once = Once::new();

// Grid layout constants - ultraclean aesthetic
const GRID_COLUMNS: f64 = 5.0;
const CELL_WIDTH: f64 = 116.0;
const CELL_HEIGHT: f64 = 100.0;
const ICON_SIZE: f64 = 48.0;
const CELL_SPACING: f64 = 12.0;

// Global config storage for hover callbacks
static CONFIG_DATA: Mutex<Option<Config>> = Mutex::new(None);

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
    prompt_label: SendId,                    // Reference to prompt label for mode switching
    mode_badge: SendId,                      // Reference to mode badge label
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
                    let raw_query = std::ffi::CStr::from_ptr(query_cstr).to_string_lossy();

                    // Detect mode from prefix and strip it
                    let (mode, query) = if raw_query.starts_with('/') {
                        (SearchMode::Files, raw_query[1..].to_string())
                    } else if raw_query.starts_with(':') {
                        (SearchMode::Run, raw_query[1..].to_string())
                    } else {
                        (SearchMode::Apps, raw_query.to_string())
                    };

                    // Update mode and UI indicators
                    *data.search_mode.lock().unwrap() = mode;
                    update_mode_ui(mode, data.prompt_label.0, data.mode_badge.0);

                    // Filter based on mode
                    let filtered: Vec<SearchResult> = match mode {
                        SearchMode::Apps => {
                            if query.is_empty() {
                                // Show first 15 apps when empty (already sorted alphabetically)
                                let apps = data.apps.lock().unwrap();
                                apps.iter()
                                    .take(15)
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

                    // Rebuild the results view
                    let results_view = data.results_view.0;
                    let config = data.config.clone();
                    rebuild_results_grid(results_view, &filtered, 0, &config);
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

                                // Exit after launching
                                let app = NSApp();
                                let _: () = msg_send![app, terminate: nil];
                            }
                        }

                        return YES as u8;
                    }

                    // Arrow Down triggers "moveDown:" - move to next row (5 items)
                    if sel_str == "moveDown:" {
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let grid_cols: usize = GRID_COLUMNS as usize;
                            let filtered_count = data.filtered.lock().unwrap().len();
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            let new_idx = *selected_idx + grid_cols;
                            // Wrap to top row (same column) if at bottom
                            if new_idx < filtered_count {
                                *selected_idx = new_idx;
                            } else {
                                // Wrap: go to same column in first row
                                *selected_idx = *selected_idx % grid_cols;
                            }
                            drop(selected_idx);

                            let results_view = data.results_view.0;
                            let filtered = data.filtered.lock().unwrap().clone();
                            let selected_index = *data.selected_index.lock().unwrap();
                            let config = data.config.clone();
                            drop(data_map);

                            rebuild_results_grid(results_view, &filtered, selected_index, &config);
                        }
                        return YES as u8;
                    }

                    // Arrow Up triggers "moveUp:" - move to previous row (5 items)
                    if sel_str == "moveUp:" {
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let grid_cols: usize = GRID_COLUMNS as usize;
                            let filtered_count = data.filtered.lock().unwrap().len();
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx >= grid_cols {
                                *selected_idx -= grid_cols;
                            } else {
                                // Wrap: go to same column in last row
                                let last_row_start = (filtered_count / grid_cols) * grid_cols;
                                let target = last_row_start + (*selected_idx % grid_cols);
                                *selected_idx = target.min(filtered_count.saturating_sub(1));
                            }
                            drop(selected_idx);

                            let results_view = data.results_view.0;
                            let filtered = data.filtered.lock().unwrap().clone();
                            let selected_index = *data.selected_index.lock().unwrap();
                            let config = data.config.clone();
                            drop(data_map);

                            rebuild_results_grid(results_view, &filtered, selected_index, &config);
                        }
                        return YES as u8;
                    }

                    // Arrow Right triggers "moveRight:" - move to next item
                    if sel_str == "moveRight:" {
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let filtered_count = data.filtered.lock().unwrap().len();
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx < filtered_count.saturating_sub(1) {
                                *selected_idx += 1;
                            } else {
                                // Wrap to first item
                                *selected_idx = 0;
                            }
                            let new_selected = *selected_idx;
                            drop(selected_idx);

                            // Update cell backgrounds for visual selection
                            let results_view = data.results_view.0;
                            let selection_bg =
                                Config::hex_to_nscolor(&data.config.colors.selection_background);
                            let selection_text =
                                Config::hex_to_nscolor(&data.config.colors.selection_text);
                            let normal_text = Config::hex_to_nscolor(&data.config.colors.text);
                            let clear_color: id = msg_send![class!(NSColor), clearColor];

                            let subviews: id = msg_send![results_view, subviews];
                            let count: usize = msg_send![subviews, count];
                            for i in 0..count {
                                let cell_view: id = msg_send![subviews, objectAtIndex: i];
                                let layer: id = msg_send![cell_view, layer];
                                if layer != nil {
                                    let row_idx: isize =
                                        *(&*cell_view as &Object).get_ivar::<isize>("rowIndex");
                                    if row_idx == new_selected as isize {
                                        let cg_color: id = msg_send![selection_bg, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    } else {
                                        let cg_color: id = msg_send![clear_color, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    }
                                    // Update label text color
                                    let cell_subviews: id = msg_send![cell_view, subviews];
                                    let cell_subview_count: usize = msg_send![cell_subviews, count];
                                    for j in 0..cell_subview_count {
                                        let subview: id = msg_send![cell_subviews, objectAtIndex: j];
                                        let class_name: id = msg_send![subview, className];
                                        let cstr: *const i8 = msg_send![class_name, UTF8String];
                                        let name = std::ffi::CStr::from_ptr(cstr).to_string_lossy();
                                        if name == "NSTextField" {
                                            let text_color = if row_idx == new_selected as isize {
                                                selection_text
                                            } else {
                                                normal_text
                                            };
                                            let _: () = msg_send![subview, setTextColor: text_color];
                                        }
                                    }
                                }
                            }
                        }
                        return YES as u8;
                    }

                    // Arrow Left triggers "moveLeft:" - move to previous item
                    if sel_str == "moveLeft:" {
                        let delegate: id = msg_send![control, delegate];
                        let delegate_ptr = delegate as usize;

                        let mut data_map = DELEGATE_DATA.lock().unwrap();
                        if let Some(data) = data_map.as_mut().and_then(|m| m.get_mut(&delegate_ptr))
                        {
                            let filtered_count = data.filtered.lock().unwrap().len();
                            let mut selected_idx = data.selected_index.lock().unwrap();
                            if *selected_idx > 0 {
                                *selected_idx -= 1;
                            } else {
                                // Wrap to last item
                                *selected_idx = filtered_count.saturating_sub(1);
                            }
                            let new_selected = *selected_idx;
                            drop(selected_idx);

                            // Update cell backgrounds for visual selection
                            let results_view = data.results_view.0;
                            let selection_bg =
                                Config::hex_to_nscolor(&data.config.colors.selection_background);
                            let selection_text =
                                Config::hex_to_nscolor(&data.config.colors.selection_text);
                            let normal_text = Config::hex_to_nscolor(&data.config.colors.text);
                            let clear_color: id = msg_send![class!(NSColor), clearColor];

                            let subviews: id = msg_send![results_view, subviews];
                            let count: usize = msg_send![subviews, count];
                            for i in 0..count {
                                let cell_view: id = msg_send![subviews, objectAtIndex: i];
                                let layer: id = msg_send![cell_view, layer];
                                if layer != nil {
                                    let row_idx: isize =
                                        *(&*cell_view as &Object).get_ivar::<isize>("rowIndex");
                                    if row_idx == new_selected as isize {
                                        let cg_color: id = msg_send![selection_bg, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    } else {
                                        let cg_color: id = msg_send![clear_color, CGColor];
                                        let _: () = msg_send![layer, setBackgroundColor: cg_color];
                                    }
                                    // Update label text color
                                    let cell_subviews: id = msg_send![cell_view, subviews];
                                    let cell_subview_count: usize = msg_send![cell_subviews, count];
                                    for j in 0..cell_subview_count {
                                        let subview: id = msg_send![cell_subviews, objectAtIndex: j];
                                        let class_name: id = msg_send![subview, className];
                                        let cstr: *const i8 = msg_send![class_name, UTF8String];
                                        let name = std::ffi::CStr::from_ptr(cstr).to_string_lossy();
                                        if name == "NSTextField" {
                                            let text_color = if row_idx == new_selected as isize {
                                                selection_text
                                            } else {
                                                normal_text
                                            };
                                            let _: () = msg_send![subview, setTextColor: text_color];
                                        }
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

                    // Apply hover background color from config
                    let layer: id = msg_send![this, layer];
                    if layer != nil {
                        // Get selection color from global config
                        let config_guard = CONFIG_DATA.lock().unwrap();
                        let hover_color = if let Some(ref config) = *config_guard {
                            Config::hex_to_nscolor(&config.colors.selection_background)
                        } else {
                            Config::hex_to_nscolor("#d79921") // Fallback
                        };
                        drop(config_guard);
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

        // Get delegate data and launch the selected item
        let window: id = msg_send![this, window];
        if window == nil {
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
            return;
        }

        let delegate: id = msg_send![text_field, delegate];
        let delegate_ptr = delegate as usize;

        let data_map = DELEGATE_DATA.lock().unwrap();
        if let Some(data) = data_map.as_ref().and_then(|m| m.get(&delegate_ptr)) {
            let filtered = data.filtered.lock().unwrap();
            if let Some(result) = filtered.get(row_index as usize) {
                match result.result_type {
                    SearchMode::Apps | SearchMode::Files => {
                        let workspace_class = class!(NSWorkspace);
                        let workspace: id = msg_send![workspace_class, sharedWorkspace];

                        let path_string = NSString::alloc(nil).init_str(&result.path);
                        let url: id = msg_send![class!(NSURL), fileURLWithPath: path_string];

                        let _: bool = msg_send![workspace, openURL: url];
                    }
                    SearchMode::Run => {
                        std::process::Command::new("sh")
                            .arg("-c")
                            .arg(&result.path)
                            .spawn()
                            .ok();
                    }
                }

                // Exit after launching
                let app = NSApp();
                let _: () = msg_send![app, terminate: nil];
            }
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

/// Updates the prompt label and mode badge based on current search mode
unsafe fn update_mode_ui(mode: SearchMode, prompt_label: id, mode_badge: id) {
    let (prompt_char, badge_text, color_hex) = match mode {
        SearchMode::Apps => (">", "[apps]", "#d65d0e"),   // Orange
        SearchMode::Files => ("/", "[files]", "#458588"), // Blue
        SearchMode::Run => (":", "[run]", "#98971a"),     // Green
    };

    // Update prompt symbol and color
    let prompt_str = NSString::alloc(nil).init_str(prompt_char);
    let _: () = msg_send![prompt_label, setStringValue: prompt_str];
    let prompt_color = Config::hex_to_nscolor(color_hex);
    let _: () = msg_send![prompt_label, setTextColor: prompt_color];

    // Update mode badge
    let badge_str = NSString::alloc(nil).init_str(badge_text);
    let _: () = msg_send![mode_badge, setStringValue: badge_str];
    let badge_color: id = msg_send![prompt_color, colorWithAlphaComponent: 0.6f64];
    let _: () = msg_send![mode_badge, setTextColor: badge_color];
}

/// Rebuilds the results grid view with the given filtered results
/// This consolidates the duplicated grid rendering code from multiple locations
unsafe fn rebuild_results_grid(
    results_view: id,
    filtered: &[SearchResult],
    selected_index: usize,
    config: &Config,
) {
    // Remove all existing subviews
    loop {
        let subviews: id = msg_send![results_view, subviews];
        let count: usize = msg_send![subviews, count];
        if count == 0 {
            break;
        }
        let subview: id = msg_send![subviews, firstObject];
        let _: () = msg_send![subview, removeFromSuperview];
    }

    // Handle empty results - show "No results found" message
    if filtered.is_empty() {
        let frame: NSRect = msg_send![results_view, frame];
        let label_width = 200.0;
        let label_height = 30.0;
        let label_frame = NSRect::new(
            NSPoint::new(
                (frame.size.width - label_width) / 2.0,
                (frame.size.height - label_height) / 2.0,
            ),
            NSSize::new(label_width, label_height),
        );
        let no_results_label: id = msg_send![class!(NSTextField), alloc];
        let no_results_label: id = msg_send![no_results_label, initWithFrame: label_frame];
        let _: () = msg_send![no_results_label, setEditable: 0u32];
        let _: () = msg_send![no_results_label, setSelectable: 0u32];
        let _: () = msg_send![no_results_label, setBordered: 0u32];
        let _: () = msg_send![no_results_label, setDrawsBackground: 0u32];
        let _: () = msg_send![no_results_label, setAlignment: 1i64]; // Center
        let text_color = Config::hex_to_nscolor(&config.colors.text);
        let _: () = msg_send![no_results_label, setTextColor: text_color];
        let font: id = msg_send![class!(NSFont), systemFontOfSize: 16.0f64];
        let _: () = msg_send![no_results_label, setFont: font];
        let no_results_str = NSString::alloc(nil).init_str("No results found");
        let _: () = msg_send![no_results_label, setStringValue: no_results_str];
        let _: () = msg_send![results_view, addSubview: no_results_label];
        return;
    }

    // Get config colors
    let selection_bg = Config::hex_to_nscolor(&config.colors.selection_background);
    let selection_text = Config::hex_to_nscolor(&config.colors.selection_text);
    let normal_text = Config::hex_to_nscolor(&config.colors.text);

    // Get workspace for icons
    let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];

    let frame: NSRect = msg_send![results_view, frame];

    // Resize results_view to fit all items in grid
    let num_items = filtered.len();
    let num_rows = ((num_items as f64) / GRID_COLUMNS).ceil();
    let new_height = (num_rows * (CELL_HEIGHT + CELL_SPACING)).max(frame.size.height);
    let new_frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(frame.size.width, new_height),
    );
    let _: () = msg_send![results_view, setFrame: new_frame];

    let container_height = new_height;
    let row_class = create_row_view_class();

    for (index, result) in filtered.iter().enumerate() {
        // Calculate grid position
        let col = (index as f64) % GRID_COLUMNS;
        let row = ((index as f64) / GRID_COLUMNS).floor();

        let x_pos = col * (CELL_WIDTH + CELL_SPACING);
        let y_pos = container_height - ((row + 1.0) * (CELL_HEIGHT + CELL_SPACING));

        // Create cell
        let cell_frame = NSRect::new(
            NSPoint::new(x_pos, y_pos),
            NSSize::new(CELL_WIDTH, CELL_HEIGHT),
        );
        let cell_view: id = msg_send![row_class, alloc];
        let cell_view: id = msg_send![cell_view, initWithFrame: cell_frame];
        let _: () = msg_send![cell_view, setWantsLayer: 1u32];

        (*cell_view).set_ivar("rowIndex", index as isize);

        let cell_layer: id = msg_send![cell_view, layer];
        let _: () = msg_send![cell_layer, setCornerRadius: 14.0f64];
        let _: () = msg_send![cell_layer, setMasksToBounds: NO];

        // Ultraclean: minimal styling, emphasis on selection
        if index == selected_index {
            // Selected: elevated card with glow
            let selected_bg = Config::hex_to_nscolor("#32302f");
            let cg_selected_bg: id = msg_send![selected_bg, CGColor];
            let _: () = msg_send![cell_layer, setBackgroundColor: cg_selected_bg];

            // Accent border
            let accent = Config::hex_to_nscolor("#fabd2f"); // Gruvbox yellow
            let cg_accent: id = msg_send![accent, CGColor];
            let _: () = msg_send![cell_layer, setBorderColor: cg_accent];
            let _: () = msg_send![cell_layer, setBorderWidth: 2.0f64];

            // Glow shadow
            let glow_color = Config::hex_to_nscolor("#fabd2f"); // Gruvbox yellow
            let cg_glow: id = msg_send![glow_color, CGColor];
            let _: () = msg_send![cell_layer, setShadowColor: cg_glow];
            let _: () = msg_send![cell_layer, setShadowOpacity: 0.4f32];
            let _: () = msg_send![cell_layer, setShadowRadius: 12.0f64];
            let shadow_offset = CGSize { width: 0.0, height: 0.0 };
            let _: () = msg_send![cell_layer, setShadowOffset: shadow_offset];
        } else {
            // Unselected: clean, no border, subtle bg
            let cell_bg = Config::hex_to_nscolor("#252423");
            let cg_cell_bg: id = msg_send![cell_bg, CGColor];
            let _: () = msg_send![cell_layer, setBackgroundColor: cg_cell_bg];
            let _: () = msg_send![cell_layer, setBorderWidth: 0.0f64];
            let _: () = msg_send![cell_layer, setShadowOpacity: 0.0f32];
        }

        // Icon centered
        let icon_x = (CELL_WIDTH - ICON_SIZE) / 2.0;
        let icon_y = CELL_HEIGHT - ICON_SIZE - 16.0;
        let icon_frame = NSRect::new(
            NSPoint::new(icon_x, icon_y),
            NSSize::new(ICON_SIZE, ICON_SIZE),
        );

        let icon: id = match result.result_type {
            SearchMode::Apps | SearchMode::Files => {
                let path_str = NSString::alloc(nil).init_str(&result.path);
                msg_send![workspace, iconForFile: path_str]
            }
            SearchMode::Run => {
                // Use Terminal app icon for commands
                let terminal_path = NSString::alloc(nil).init_str("/System/Applications/Utilities/Terminal.app");
                msg_send![workspace, iconForFile: terminal_path]
            }
        };

        let icon_ns_size = NSSize::new(ICON_SIZE, ICON_SIZE);
        let _: () = msg_send![icon, setSize: icon_ns_size];
        let icon_view: id = msg_send![class!(NSImageView), alloc];
        let icon_view: id = msg_send![icon_view, initWithFrame: icon_frame];
        let _: () = msg_send![icon_view, setImage: icon];
        let _: () = msg_send![icon_view, setImageScaling: 3i64];
        let _: () = msg_send![cell_view, addSubview: icon_view];

        // Label position depends on whether we show path hint
        let has_path_hint = result.result_type == SearchMode::Files;
        let label_y = if has_path_hint { 16.0 } else { 6.0 };

        // Label - clean, readable
        let label_frame = NSRect::new(NSPoint::new(4.0, label_y), NSSize::new(CELL_WIDTH - 8.0, 18.0));
        let label: id = msg_send![class!(NSTextField), alloc];
        let label: id = msg_send![label, initWithFrame: label_frame];
        let _: () = msg_send![label, setEditable: 0u32];
        let _: () = msg_send![label, setSelectable: 0u32];
        let _: () = msg_send![label, setBordered: 0u32];
        let _: () = msg_send![label, setDrawsBackground: 0u32];
        let _: () = msg_send![label, setAlignment: 1i64];

        // Text color: bright when selected, muted otherwise
        let text_color = if index == selected_index {
            Config::hex_to_nscolor("#fbf1c7") // Gruvbox light
        } else {
            Config::hex_to_nscolor("#a89984") // Gruvbox gray (brighter)
        };
        let _: () = msg_send![label, setTextColor: text_color];

        // Readable font size
        let font_cls = class!(NSFont);
        let font: id = msg_send![font_cls, systemFontOfSize:12.0f64 weight:0.0f64];
        let _: () = msg_send![label, setFont: font];
        let name_str = NSString::alloc(nil).init_str(&result.name);
        let _: () = msg_send![label, setStringValue: name_str];
        let _: () = msg_send![label, setLineBreakMode: 4i64];

        let _: () = msg_send![cell_view, addSubview: label];

        // Path hint for files (below filename)
        if has_path_hint {
            // Truncate path: ~/Documents/foo.txt -> ~/Doc...
            let path = &result.path;
            let home = std::env::var("HOME").unwrap_or_default();
            let display_path = if path.starts_with(&home) {
                format!("~{}", &path[home.len()..])
            } else {
                path.clone()
            };
            // Get parent directory and truncate
            let parent = std::path::Path::new(&display_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let truncated = if parent.len() > 12 {
                format!("{}...", &parent[..9])
            } else {
                parent
            };

            let hint_frame = NSRect::new(NSPoint::new(4.0, 2.0), NSSize::new(CELL_WIDTH - 8.0, 14.0));
            let hint_label: id = msg_send![class!(NSTextField), alloc];
            let hint_label: id = msg_send![hint_label, initWithFrame: hint_frame];
            let _: () = msg_send![hint_label, setEditable: 0u32];
            let _: () = msg_send![hint_label, setSelectable: 0u32];
            let _: () = msg_send![hint_label, setBordered: 0u32];
            let _: () = msg_send![hint_label, setDrawsBackground: 0u32];
            let _: () = msg_send![hint_label, setAlignment: 1i64];
            let hint_color = Config::hex_to_nscolor("#665c54"); // Muted
            let _: () = msg_send![hint_label, setTextColor: hint_color];
            let hint_font: id = msg_send![font_cls, systemFontOfSize:10.0f64 weight:0.0f64];
            let _: () = msg_send![hint_label, setFont: hint_font];
            let hint_str = NSString::alloc(nil).init_str(&truncated);
            let _: () = msg_send![hint_label, setStringValue: hint_str];
            let _: () = msg_send![hint_label, setLineBreakMode: 4i64];
            let _: () = msg_send![cell_view, addSubview: hint_label];
        }

        let _: () = msg_send![results_view, addSubview: cell_view];
    }

    // Scroll to top after rebuilding
    let scroll_view: id = msg_send![results_view, enclosingScrollView];
    if scroll_view != nil {
        let clip_view: id = msg_send![scroll_view, contentView];
        let clip_bounds: NSRect = msg_send![clip_view, bounds];
        let doc_frame: NSRect = msg_send![results_view, frame];
        let scroll_point = NSPoint::new(
            0.0,
            (doc_frame.size.height - clip_bounds.size.height).max(0.0),
        );
        let _: () = msg_send![results_view, scrollPoint: scroll_point];
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
            // Initialize global config for hover callbacks
            {
                let mut config_guard = CONFIG_DATA.lock().unwrap();
                *config_guard = Some(config.clone());
            }

            let apps = Arc::new(Mutex::new(apps.clone()));

            // Get actual window dimensions
            let window_frame: NSRect = msg_send![window, frame];
            let window_width = window_frame.size.width;
            let window_height = window_frame.size.height;

            // Ultraclean search bar
            let search_height = 52.0;
            let search_padding = 28.0;
            let search_container_frame = NSRect::new(
                NSPoint::new(0.0, window_height - search_height),
                NSSize::new(window_width, search_height),
            );

            // Create search container view
            let search_container: id = msg_send![class!(NSView), alloc];
            let search_container: id =
                msg_send![search_container, initWithFrame: search_container_frame];
            let _: () = msg_send![search_container, setWantsLayer: 1u32];

            // Same as window bg - seamless
            let search_layer: id = msg_send![search_container, layer];

            // Bottom separator line
            let separator_frame = NSRect::new(
                NSPoint::new(search_padding, 0.0),
                NSSize::new(window_width - (search_padding * 2.0), 1.0),
            );
            let separator: id = msg_send![class!(NSView), alloc];
            let separator: id = msg_send![separator, initWithFrame: separator_frame];
            let _: () = msg_send![separator, setWantsLayer: 1u32];
            let sep_layer: id = msg_send![separator, layer];
            let sep_color = Config::hex_to_nscolor("#3c3836");
            let cg_sep: id = msg_send![sep_color, CGColor];
            let _: () = msg_send![sep_layer, setBackgroundColor: cg_sep];
            let _: () = msg_send![search_container, addSubview: separator];

            // Clean system font for search
            let font_size = 18.0f64;
            let font_cls = class!(NSFont);
            let search_font: id = msg_send![font_cls, systemFontOfSize:font_size weight:-0.3f64];

            // Prompt - matches search font
            let prompt_width = 20.0;
            let prompt_frame = NSRect::new(
                NSPoint::new(search_padding, (search_height - 24.0) / 2.0),
                NSSize::new(prompt_width, 24.0),
            );
            let prompt_label: id = msg_send![class!(NSTextField), alloc];
            let prompt_label: id = msg_send![prompt_label, initWithFrame: prompt_frame];
            let _: () = msg_send![prompt_label, setEditable: 0u32];
            let _: () = msg_send![prompt_label, setSelectable: 0u32];
            let _: () = msg_send![prompt_label, setBordered: 0u32];
            let _: () = msg_send![prompt_label, setDrawsBackground: 0u32];
            let prompt_str = NSString::alloc(nil).init_str(">");
            let _: () = msg_send![prompt_label, setStringValue: prompt_str];
            let prompt_color = Config::hex_to_nscolor("#d65d0e"); // Gruvbox orange
            let _: () = msg_send![prompt_label, setTextColor: prompt_color];
            let _: () = msg_send![prompt_label, setFont: search_font];
            let _: () = msg_send![search_container, addSubview: prompt_label];

            // Mode badge (right-aligned)
            let badge_width = 50.0;
            let badge_frame = NSRect::new(
                NSPoint::new(window_width - search_padding - badge_width, (search_height - 20.0) / 2.0),
                NSSize::new(badge_width, 20.0),
            );
            let mode_badge: id = msg_send![class!(NSTextField), alloc];
            let mode_badge: id = msg_send![mode_badge, initWithFrame: badge_frame];
            let _: () = msg_send![mode_badge, setEditable: 0u32];
            let _: () = msg_send![mode_badge, setSelectable: 0u32];
            let _: () = msg_send![mode_badge, setBordered: 0u32];
            let _: () = msg_send![mode_badge, setDrawsBackground: 0u32];
            let _: () = msg_send![mode_badge, setAlignment: 2i64]; // Right align
            let badge_str = NSString::alloc(nil).init_str("[apps]");
            let _: () = msg_send![mode_badge, setStringValue: badge_str];
            let badge_color: id = msg_send![prompt_color, colorWithAlphaComponent: 0.6f64];
            let _: () = msg_send![mode_badge, setTextColor: badge_color];
            let badge_font: id = msg_send![font_cls, systemFontOfSize:12.0f64 weight:0.0f64];
            let _: () = msg_send![mode_badge, setFont: badge_font];
            let _: () = msg_send![search_container, addSubview: mode_badge];

            // Create text field after prompt (leave room for badge)
            let text_field_x = search_padding + prompt_width + 8.0;
            let text_field_width = window_width - text_field_x - search_padding - badge_width - 8.0;
            let text_field_height = 24.0;
            let text_field_y = (search_height - text_field_height) / 2.0;

            let search_frame = NSRect::new(
                NSPoint::new(text_field_x, text_field_y),
                NSSize::new(text_field_width, text_field_height),
            );

            let search_field_alloc = NSTextField::alloc(nil);
            let search_field: id = msg_send![search_field_alloc, initWithFrame: search_frame];

            // Minimal placeholder
            let placeholder_text = NSString::alloc(nil).init_str("type to search");
            let placeholder_color = Config::hex_to_nscolor("#504945");
            let attrs_dict: id = msg_send![class!(NSMutableDictionary), new];
            let foreground_key = NSString::alloc(nil).init_str("NSColor");
            let _: () = msg_send![attrs_dict, setObject:placeholder_color forKey:foreground_key];
            let font_key = NSString::alloc(nil).init_str("NSFont");
            let _: () = msg_send![attrs_dict, setObject:search_font forKey:font_key];

            let placeholder_attr: id = msg_send![class!(NSAttributedString), alloc];
            let placeholder_attr: id =
                msg_send![placeholder_attr, initWithString:placeholder_text attributes:attrs_dict];
            let _: () = msg_send![search_field, setPlaceholderAttributedString: placeholder_attr];

            let _: () = msg_send![search_field, setBezeled: 0u32];
            let _: () = msg_send![search_field, setBordered: 0u32];
            let _: () = msg_send![search_field, setEditable: 1u32];
            let _: () = msg_send![search_field, setSelectable: 1u32];
            let _: () = msg_send![search_field, setDrawsBackground: 0u32];
            let _: () = msg_send![search_field, setFocusRingType: 1u32];

            // Warm text color
            let text_color = Config::hex_to_nscolor("#ebdbb2");
            let _: () = msg_send![search_field, setTextColor: text_color];
            let _: () = msg_send![search_field, setFont: search_font];

            // Configure cell
            let _: () = msg_send![search_field, setAlignment: 0i64];
            let cell: id = msg_send![search_field, cell];
            let _: () = msg_send![cell, setUsesSingleLineMode: 1u32];
            let _: () = msg_send![cell, setScrollable: 1u32];
            let _: () = msg_send![cell, setLineBreakMode: 4i64];
            let _: () = msg_send![cell, setFocusRingType: 1u32];
            let _: () = msg_send![search_field, setRefusesFirstResponder: 0u32];

            let _: () = msg_send![search_container, addSubview: search_field];

            // Add search container to window
            let content_view: id = msg_send![window, contentView];
            let _: () = msg_send![content_view, addSubview: search_container];

            // No count label - clean style
            let pill_buttons: Vec<SendId> = Vec::new();

            // Grid layout
            let results_padding = 28.0;
            let hints_area = 28.0;

            let results_container_frame = NSRect::new(
                NSPoint::new(results_padding, hints_area),
                NSSize::new(
                    window_width - (results_padding * 2.0),
                    window_height - search_height - hints_area - 12.0,
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

            // Show first 15 apps initially (3 rows x 5 columns, sorted alphabetically)
            let apps_locked = apps.lock().unwrap();
            let initial_apps: Vec<SearchResult> = apps_locked
                .iter()
                .take(15)
                .map(|app| SearchResult::new(app.name.clone(), app.path.clone(), SearchMode::Apps))
                .collect();
            drop(apps_locked);

            // Use shared rebuild function for initial grid
            rebuild_results_grid(results_view, &initial_apps, 0, &config);

            let _: () = msg_send![content_view, addSubview: scroll_view];

            // Minimal hints at bottom
            let hints_height = 16.0;
            let hints_frame = NSRect::new(
                NSPoint::new(0.0, 6.0),
                NSSize::new(window_width, hints_height),
            );
            let hints_label: id = msg_send![class!(NSTextField), alloc];
            let hints_label: id = msg_send![hints_label, initWithFrame: hints_frame];
            let _: () = msg_send![hints_label, setEditable: 0u32];
            let _: () = msg_send![hints_label, setSelectable: 0u32];
            let _: () = msg_send![hints_label, setBordered: 0u32];
            let _: () = msg_send![hints_label, setDrawsBackground: 0u32];
            let _: () = msg_send![hints_label, setAlignment: 1i64];
            let hints_color = Config::hex_to_nscolor("#504945");
            let _: () = msg_send![hints_label, setTextColor: hints_color];
            let hints_font: id = msg_send![class!(NSFont), systemFontOfSize: 11.0f64];
            let _: () = msg_send![hints_label, setFont: hints_font];
            let hints_str = NSString::alloc(nil).init_str("enter · open   esc · close   / files   : run");
            let _: () = msg_send![hints_label, setStringValue: hints_str];
            let _: () = msg_send![content_view, addSubview: hints_label];

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
                    prompt_label: SendId(prompt_label),
                    mode_badge: SendId(mode_badge),
                },
            );
            drop(data_map); // Release the lock

            // Set delegate on search field
            let _: () = msg_send![search_field, setDelegate: delegate];

            // Force window to be key and make search field first responder
            let _: () = msg_send![window, makeKeyAndOrderFront: nil];
            let _: bool = msg_send![window, makeFirstResponder: search_field];

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
