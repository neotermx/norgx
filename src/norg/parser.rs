/// Block-level token — one per line.
#[derive(Debug, PartialEq, Clone)]
pub enum BlockToken {
    Heading { level: u8, text: String },
    TodoOpen(String),
    TodoDone(String),
    TodoPending(String),
    CodeFenceOpen { lang: String },
    CodeFenceClose,
    PlainText(String),
    Blank,
}

/// Inline span within a line of plain text.
#[derive(Debug, PartialEq, Clone)]
pub enum InlineToken {
    Bold(String),
    Italic(String),
    Link { path: String, label: Option<String> },
    Plain(String),
}

pub fn parse_line(line: &str) -> BlockToken {
    if line.trim().is_empty() {
        return BlockToken::Blank;
    }

    // Headings: one or more '*' followed by a space
    if line.starts_with('*') {
        let level = line.chars().take_while(|&c| c == '*').count();
        let rest = &line[level..];
        if rest.starts_with(' ') {
            return BlockToken::Heading {
                level: (level as u8).min(6),
                text: rest[1..].to_string(),
            };
        }
    }

    // TODO items — match after any leading whitespace so indented todos render correctly
    let trimmed = line.trim_start();
    if let Some(text) = trimmed.strip_prefix("- ( ) ") {
        return BlockToken::TodoOpen(text.to_string());
    }
    if let Some(text) = trimmed.strip_prefix("- (x) ") {
        return BlockToken::TodoDone(text.to_string());
    }
    if let Some(text) = trimmed.strip_prefix("- (-) ") {
        return BlockToken::TodoPending(text.to_string());
    }

    // Norg code fences: @code [lang] ... @end
    if let Some(rest) = line.strip_prefix("@code") {
        return BlockToken::CodeFenceOpen {
            lang: rest.trim().to_string(),
        };
    }
    if line == "@end" {
        return BlockToken::CodeFenceClose;
    }

    BlockToken::PlainText(line.to_string())
}

/// Parse inline tokens within a plain-text line.
pub fn parse_inline(text: &str) -> Vec<InlineToken> {
    let mut tokens: Vec<InlineToken> = Vec::new();
    let mut current = String::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Bold: *text*
        if bytes[i] == b'*' {
            if let Some(close) = text[i + 1..].find('*') {
                let inner = &text[i + 1..i + 1 + close];
                if !inner.is_empty() {
                    flush(&mut current, &mut tokens);
                    tokens.push(InlineToken::Bold(inner.to_string()));
                    i += 1 + close + 1;
                    continue;
                }
            }
        }

        // Italic: /text/
        if bytes[i] == b'/' {
            if let Some(close) = text[i + 1..].find('/') {
                let inner = &text[i + 1..i + 1 + close];
                if !inner.is_empty() {
                    flush(&mut current, &mut tokens);
                    tokens.push(InlineToken::Italic(inner.to_string()));
                    i += 1 + close + 1;
                    continue;
                }
            }
        }

        // Link: {:path:}[label]
        if text[i..].starts_with("{:") {
            if let Some(close) = text[i + 2..].find(":}") {
                let path = text[i + 2..i + 2 + close].to_string();
                let after = i + 2 + close + 2;
                let label = if after < len && bytes[after] == b'[' {
                    text[after + 1..].find(']').map(|end| {
                        text[after + 1..after + 1 + end].to_string()
                    })
                } else {
                    None
                };
                let consumed = after + label.as_ref().map(|l| l.len() + 2).unwrap_or(0);
                flush(&mut current, &mut tokens);
                tokens.push(InlineToken::Link { path, label });
                i = consumed;
                continue;
            }
        }

        let c = text[i..].chars().next().unwrap();
        current.push(c);
        i += c.len_utf8();
    }

    flush(&mut current, &mut tokens);
    tokens
}

fn flush(buf: &mut String, out: &mut Vec<InlineToken>) {
    if !buf.is_empty() {
        out.push(InlineToken::Plain(std::mem::take(buf)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings() {
        assert_eq!(
            parse_line("* Hello"),
            BlockToken::Heading { level: 1, text: "Hello".into() }
        );
        assert_eq!(
            parse_line("*** Section"),
            BlockToken::Heading { level: 3, text: "Section".into() }
        );
    }

    #[test]
    fn todos() {
        assert_eq!(parse_line("- ( ) buy milk"), BlockToken::TodoOpen("buy milk".into()));
        assert_eq!(parse_line("- (x) done"), BlockToken::TodoDone("done".into()));
        assert_eq!(parse_line("- (-) in progress"), BlockToken::TodoPending("in progress".into()));
        // Indented todos
        assert_eq!(parse_line("  - ( ) indented"), BlockToken::TodoOpen("indented".into()));
        assert_eq!(parse_line("\t- (x) tabbed"), BlockToken::TodoDone("tabbed".into()));
    }

    #[test]
    fn blank() {
        assert_eq!(parse_line(""), BlockToken::Blank);
        assert_eq!(parse_line("   "), BlockToken::Blank);
    }

    #[test]
    fn inline_bold_italic() {
        let tokens = parse_inline("hello *world* and /there/");
        assert_eq!(tokens, vec![
            InlineToken::Plain("hello ".into()),
            InlineToken::Bold("world".into()),
            InlineToken::Plain(" and ".into()),
            InlineToken::Italic("there".into()),
        ]);
    }
}
