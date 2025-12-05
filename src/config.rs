use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use cocoa::base::id;
use cocoa::appkit::NSColor;
use objc::{msg_send, sel, sel_impl, class};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub window: WindowConfig,
    pub colors: ColorConfig,
    pub font: FontConfig,
    pub theme: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColorConfig {
    pub background: String,
    pub text: String,
    pub selection_background: String,
    pub selection_text: String,
    pub input_background: String,
    pub border: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FontConfig {
    pub size: f64,
    pub family: String,
}

impl Config {
    pub fn load() -> Self {
        let config_path = Self::config_path();

        if let Ok(contents) = fs::read_to_string(&config_path) {
            if let Ok(config) = serde_json::from_str(&contents) {
                return config;
            }
        }

        // Check environment variable for theme
        let theme = std::env::var("ROFI_THEME").unwrap_or_else(|_| "gruvbox".to_string());

        match theme.as_str() {
            "8bit" => Self::theme_8bit(),
            "catppuccin" => Self::theme_catppuccin(),
            _ => Self::theme_gruvbox(),
        }
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap()
            .join("rofi-mac")
            .join("config.json")
    }

    pub fn theme_gruvbox() -> Self {
        Config {
            window: WindowConfig {
                width: 650,
                height: 450,
            },
            colors: ColorConfig {
                background: "#1e1b2e".to_string(),      // Dark purple-blue background
                text: "#e0e0e0".to_string(),            // Light text for contrast
                selection_background: "#d946ef".to_string(), // Magenta/pink selection
                selection_text: "#ffffff".to_string(),
                input_background: "#2a2640".to_string(), // Slightly lighter purple
                border: "#3a3a3a".to_string(),
            },
            font: FontConfig {
                size: 16.0,                              // Slightly smaller for cleaner look
                family: "SF Pro Display".to_string(),   // Modern macOS system font
            },
            theme: "gruvbox".to_string(),
        }
    }

    pub fn theme_8bit() -> Self {
        Config {
            window: WindowConfig {
                width: 500,
                height: 350,
            },
            colors: ColorConfig {
                background: "#000000".to_string(),
                text: "#00ff00".to_string(),
                selection_background: "#00aa00".to_string(),
                selection_text: "#000000".to_string(),
                input_background: "#001100".to_string(),
                border: "#00ff00".to_string(),
            },
            font: FontConfig {
                size: 18.0,
                family: "Monaco".to_string(),
            },
            theme: "8bit".to_string(),
        }
    }

    pub fn theme_catppuccin() -> Self {
        Config {
            window: WindowConfig {
                width: 500,
                height: 350,
            },
            colors: ColorConfig {
                background: "#1e1e2e".to_string(),
                text: "#cdd6f4".to_string(),
                selection_background: "#89b4fa".to_string(),
                selection_text: "#1e1e2e".to_string(),
                input_background: "#313244".to_string(),
                border: "#89b4fa".to_string(),
            },
            font: FontConfig {
                size: 18.0,
                family: "Monaco".to_string(),
            },
            theme: "catppuccin".to_string(),
        }
    }

    pub fn get_bg_color(&self) -> id {
        unsafe {
            Self::hex_to_nscolor(&self.colors.background)
        }
    }

    pub fn get_text_color(&self) -> id {
        unsafe {
            Self::hex_to_nscolor(&self.colors.text)
        }
    }

    pub fn get_selection_color(&self) -> id {
        unsafe {
            Self::hex_to_nscolor(&self.colors.selection_background)
        }
    }

    pub unsafe fn hex_to_nscolor(hex: &str) -> id {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64 / 255.0;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f64 / 255.0;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f64 / 255.0;

        let cls = class!(NSColor);
        msg_send![cls, colorWithRed:r green:g blue:b alpha:1.0]
    }
}
