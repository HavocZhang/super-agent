use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

pub struct MarkdownRenderer {
    syntax_set: SyntaxSet,
    theme: syntect::highlighting::Theme,
}

unsafe impl Send for MarkdownRenderer {}
unsafe impl Sync for MarkdownRenderer {}

impl MarkdownRenderer {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap_or_else(|| theme_set.themes.values().next().cloned().unwrap());
        Self { syntax_set, theme }
    }

    pub fn render(&self, markdown: &str) -> Text<'static> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut current_spans: Vec<Span<'static>> = Vec::new();
        let mut in_code_block = false;
        let mut code_lang = String::new();
        let mut code_buf = String::new();
        let mut style_stack = StyleStack::new();

        let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
        let parser = Parser::new_ext(markdown, options);

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::CodeBlock(kind) => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        in_code_block = true;
                        code_lang = match kind {
                            pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                            pulldown_cmark::CodeBlockKind::Indented => String::new(),
                        };
                        code_buf.clear();
                    }
                    Tag::Heading { level, .. } => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        if !lines.is_empty() {
                            lines.push(Line::default());
                        }
                        style_stack.heading = Some(level as u8);
                    }
                    Tag::Emphasis => style_stack.italic = true,
                    Tag::Strong => style_stack.bold = true,
                    Tag::Strikethrough => style_stack.strikethrough = true,
                    Tag::BlockQuote(_) => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        current_spans.push(Span::styled(
                            "│ ".to_string(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    Tag::List(Some(start)) => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        style_stack.ordered_list = Some(start);
                    }
                    Tag::List(None) => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        style_stack.ordered_list = None;
                    }
                    Tag::Item => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        if let Some(ref mut idx) = style_stack.ordered_list {
                            current_spans.push(Span::styled(
                                format!("{}. ", idx),
                                Style::default().fg(Color::Cyan),
                            ));
                            *idx += 1;
                        } else {
                            current_spans.push(Span::styled(
                                "• ".to_string(),
                                Style::default().fg(Color::Yellow),
                            ));
                        }
                    }
                    Tag::Link { dest_url, .. } => {
                        current_spans.push(Span::styled(
                            "[".to_string(),
                            Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
                        ));
                        style_stack.link_url = Some(dest_url.to_string());
                    }
                    Tag::Paragraph => {
                        self.flush_spans(&mut lines, &mut current_spans);
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::CodeBlock => {
                        if in_code_block {
                            let highlighted = self.highlight_code(
                                &code_buf,
                                if code_lang.is_empty() { None } else { Some(&code_lang) },
                            );
                            lines.extend(highlighted);
                            in_code_block = false;
                            code_lang.clear();
                            code_buf.clear();
                        }
                    }
                    TagEnd::Heading(_) => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        lines.push(Line::default());
                        style_stack.heading = None;
                    }
                    TagEnd::Emphasis => style_stack.italic = false,
                    TagEnd::Strong => style_stack.bold = false,
                    TagEnd::Strikethrough => style_stack.strikethrough = false,
                    TagEnd::BlockQuote(_) => {
                        self.flush_spans(&mut lines, &mut current_spans);
                    }
                    TagEnd::List(_) => {
                        self.flush_spans(&mut lines, &mut current_spans);
                        style_stack.ordered_list = None;
                    }
                    TagEnd::Item => {
                        self.flush_spans(&mut lines, &mut current_spans);
                    }
                    TagEnd::Link => {
                        if let Some(url) = style_stack.link_url.take() {
                            current_spans.push(Span::styled(
                                format!("]({})", url),
                                Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
                            ));
                        }
                    }
                    TagEnd::Paragraph => {
                        self.flush_spans(&mut lines, &mut current_spans);
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if in_code_block {
                        code_buf.push_str(&text);
                    } else {
                        let style = style_stack.current_style();
                        current_spans.push(Span::styled(text.to_string(), style));
                    }
                }
                Event::Code(code) => {
                    current_spans.push(Span::styled(
                        code.to_string(),
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::ITALIC),
                    ));
                }
                Event::SoftBreak => {
                    if in_code_block {
                        code_buf.push('\n');
                    } else {
                        current_spans.push(Span::raw(" ".to_string()));
                    }
                }
                Event::HardBreak => {
                    if in_code_block {
                        code_buf.push('\n');
                    } else {
                        self.flush_spans(&mut lines, &mut current_spans);
                    }
                }
                Event::Rule => {
                    self.flush_spans(&mut lines, &mut current_spans);
                    lines.push(Line::from(Span::styled(
                        "─".repeat(60),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                _ => {}
            }
        }

        self.flush_spans(&mut lines, &mut current_spans);

        if lines.is_empty() {
            lines.push(Line::default());
        }

        Text::from(lines)
    }

    fn flush_spans(&self, lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>) {
        if !spans.is_empty() {
            lines.push(Line::from(std::mem::take(spans)));
        }
    }

    fn highlight_code(&self, code: &str, lang: Option<&str>) -> Vec<Line<'static>> {
        let syntax = lang
            .and_then(|l| self.syntax_set.find_syntax_by_token(l))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut lines = Vec::new();

        for line in code.lines() {
            let ranges = highlighter.highlight_line(line, &self.syntax_set).unwrap_or_default();
            let spans: Vec<Span<'static>> = ranges
                .into_iter()
                .map(|(style, text)| {
                    let fg = style.foreground;
                    let color = Color::Rgb(fg.r, fg.g, fg.b);
                    Span::styled(text.to_string(), Style::default().fg(color))
                })
                .collect();
            lines.push(Line::from(spans));
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }
        lines
    }
}

struct StyleStack {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    heading: Option<u8>,
    ordered_list: Option<u64>,
    link_url: Option<String>,
}

impl StyleStack {
    fn new() -> Self {
        Self {
            bold: false,
            italic: false,
            strikethrough: false,
            heading: None,
            ordered_list: None,
            link_url: None,
        }
    }

    fn current_style(&self) -> Style {
        let mut style = Style::default();

        if let Some(level) = self.heading {
            let color = match level {
                1 => Color::Cyan,
                2 => Color::Blue,
                _ => Color::Green,
            };
            style = style.fg(color).add_modifier(Modifier::BOLD);
        }

        let mut mods = Modifier::empty();
        if self.bold {
            mods |= Modifier::BOLD;
        }
        if self.italic {
            mods |= Modifier::ITALIC;
        }
        if self.strikethrough {
            mods |= Modifier::CROSSED_OUT;
        }
        style = style.add_modifier(mods);

        style
    }
}
