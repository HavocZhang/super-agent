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

pub fn family_color(family: crate::tui::tool_block::ToolFamily) -> Color {
    match family {
        crate::tui::tool_block::ToolFamily::Read => Color::Magenta,
        crate::tui::tool_block::ToolFamily::Patch => Color::Yellow,
        crate::tui::tool_block::ToolFamily::Run => Color::Cyan,
        crate::tui::tool_block::ToolFamily::Find => Color::Blue,
        crate::tui::tool_block::ToolFamily::Delegate => Color::Green,
        crate::tui::tool_block::ToolFamily::Think => Color::DarkGray,
        crate::tui::tool_block::ToolFamily::Generic => Color::DarkGray,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::tool_block::ToolFamily;

    #[test]
    fn test_tool_color() {
        assert_eq!(tool_color("file_write"), TOOL_CREATE);
        assert_eq!(tool_color("file_edit"), TOOL_EDIT);
        assert_eq!(tool_color("file_read"), TOOL_READ);
        assert_eq!(tool_color("shell"), TOOL_RUN);
        assert_eq!(tool_color("grep"), TOOL_SEARCH);
        assert_eq!(tool_color("glob"), TOOL_SEARCH);
        assert_eq!(tool_color("git_commit"), TOOL_RUN);
        assert_eq!(tool_color("git_diff"), TOOL_RUN);
        assert_eq!(tool_color("git_status"), TOOL_RUN);
        assert_eq!(tool_color("unknown_tool"), TOOL_DEFAULT);
    }

    #[test]
    fn test_context_bar_color() {
        assert_eq!(context_bar_color(0), PRIMARY);
        assert_eq!(context_bar_color(50), PRIMARY);
        assert_eq!(context_bar_color(79), PRIMARY);
        assert_eq!(context_bar_color(80), WARNING);
        assert_eq!(context_bar_color(94), WARNING);
        assert_eq!(context_bar_color(95), ERROR);
        assert_eq!(context_bar_color(100), ERROR);
    }

    #[test]
    fn test_family_color() {
        assert_eq!(family_color(ToolFamily::Read), Color::Magenta);
        assert_eq!(family_color(ToolFamily::Patch), Color::Yellow);
        assert_eq!(family_color(ToolFamily::Run), Color::Cyan);
        assert_eq!(family_color(ToolFamily::Find), Color::Blue);
        assert_eq!(family_color(ToolFamily::Delegate), Color::Green);
        assert_eq!(family_color(ToolFamily::Think), Color::DarkGray);
        assert_eq!(family_color(ToolFamily::Generic), Color::DarkGray);
    }
}
