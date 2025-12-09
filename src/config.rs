use cocoa::appkit::NSColor;
use cocoa::base::id;
use objc::{class, msg_send, sel, sel_impl};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
            "modern" => Self::theme_modern(),
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
                width: 700,
                height: 500,
            },
            colors: ColorConfig {
                background: "#282828".to_string(), // Gruvbox dark background
                text: "#ebdbb2".to_string(),       // Gruvbox light foreground
                selection_background: "#d79921".to_string(), // Gruvbox yellow/gold
                selection_text: "#282828".to_string(), // Dark text on selection
                input_background: "#3c3836".to_string(), // Gruvbox dark1
                border: "#504945".to_string(),     // Gruvbox dark4
            },
            font: FontConfig {
                size: 18.0,                           // Larger for better readability
                family: "JetBrains Mono".to_string(), // Monospace for unixporn aesthetic
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

    pub fn theme_modern() -> Self {
        Config {
            window: WindowConfig {
                width: 500,
                height: 350,
            },
            colors: ColorConfig {
                // Clean white/light background for content area
                background: "#f5f5f5".to_string(),
                // Dark text for light background
                text: "#2d2d2d".to_string(),
                // Blue selection like macOS
                selection_background: "#4a90e2".to_string(),
                selection_text: "#ffffff".to_string(),
                // Tan/brown search bar matching reference image
                input_background: "#c9a88a".to_string(),
                // Border color
                border: "#d0d0d0".to_string(),
            },
            font: FontConfig {
                size: 16.0,
                family: "SF Pro Display".to_string(), // macOS system font
            },
            theme: "modern".to_string(),
        }
    }

    pub fn get_bg_color(&self) -> id {
        unsafe { Self::hex_to_nscolor(&self.colors.background) }
    }

    pub fn get_text_color(&self) -> id {
        unsafe { Self::hex_to_nscolor(&self.colors.text) }
    }

    pub fn get_selection_color(&self) -> id {
        unsafe { Self::hex_to_nscolor(&self.colors.selection_background) }
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
