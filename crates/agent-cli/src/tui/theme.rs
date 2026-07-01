use ratatui::style::Color;

pub const PRIMARY: Color = Color::Cyan;
pub const SECONDARY: Color = Color::Blue;
pub const SUCCESS: Color = Color::Green;
pub const WARNING: Color = Color::Yellow;
pub const ERROR: Color = Color::Red;
pub const TEXT: Color = Color::White;
pub const TEXT_DIM: Color = Color::DarkGray;
pub const TEXT_MUTED: Color = Color::Gray;
pub const SURFACE: Color = Color::Black;
pub const BORDER: Color = Color::DarkGray;
pub const SPINNER: Color = Color::Cyan;

pub const TOOL_CREATE: Color = Color::Green;
pub const TOOL_EDIT: Color = Color::Yellow;
pub const TOOL_DELETE: Color = Color::Red;
pub const TOOL_READ: Color = Color::Magenta;
pub const TOOL_RUN: Color = Color::Cyan;
pub const TOOL_SEARCH: Color = Color::Blue;
pub const TOOL_DEFAULT: Color = Color::DarkGray;

pub fn tool_color(tool_name: &str) -> Color {
    match tool_name {
        "file_write" => TOOL_CREATE,
        "file_edit" => TOOL_EDIT,
        "file_read" => TOOL_READ,
        "shell" => TOOL_RUN,
        "grep" | "glob" => TOOL_SEARCH,
        "git_commit" | "git_diff" | "git_status" => TOOL_RUN,
        _ => TOOL_DEFAULT,
    }
}

pub fn context_bar_color(pct: u8) -> Color {
    if pct >= 95 {
        ERROR
    } else if pct >= 80 {
        WARNING
    } else {
        PRIMARY
    }
}
