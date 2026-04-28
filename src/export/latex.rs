use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::config::{PdfConfig, UiConfig};
use crate::norg::parser::{parse_inline, parse_line, BlockToken, InlineToken};

// ── Public API ────────────────────────────────────────────────────────────────

/// Write a `.tex` file and run xelatex on it. Returns the path to the PDF.
pub fn export_to_pdf(
    lines: &[&str],
    file_stem: &str,
    pdf: &PdfConfig,
    ui: &UiConfig,
    bw: bool,
) -> Result<PathBuf> {
    let tex_content = to_latex(lines, pdf, ui, bw);

    let tmp_dir = std::env::temp_dir().join("norgx");
    std::fs::create_dir_all(&tmp_dir)?;

    let safe_stem: String = file_stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let safe_stem = if safe_stem.is_empty() { "norgx_export".to_string() } else { safe_stem };

    let tex_path = tmp_dir.join(format!("{safe_stem}.tex"));
    let pdf_path = tmp_dir.join(format!("{safe_stem}.pdf"));

    std::fs::write(&tex_path, &tex_content)?;

    let status = std::process::Command::new("xelatex")
        .arg("-interaction=nonstopmode")
        .arg("-output-directory")
        .arg(&tmp_dir)
        .arg(&tex_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow!("xelatex not found: {e}"))?;

    // Treat as success if the PDF was produced, even with warnings/recoverable errors.
    if !pdf_path.exists() {
        let log_path = tmp_dir.join(format!("{safe_stem}.log"));
        return Err(anyhow!(
            "xelatex {} — see {}",
            if status.success() { "produced no PDF" } else { "failed" },
            log_path.display()
        ));
    }

    Ok(pdf_path)
}

/// Build a full XeLaTeX document from Norg source lines.
pub fn to_latex(lines: &[&str], pdf: &PdfConfig, ui: &UiConfig, bw: bool) -> String {
    let leading = pdf.font_size * 1.2;
    let (color_defs, heading_cmds) = build_heading_preamble(ui, bw);
    let body = translate_body(lines);

    // bookmarks=false avoids hyperref trying to extract \textcolor from heading titles.
    format!(
        r#"\documentclass{{article}}
\usepackage{{fontspec}}
\usepackage[margin={margin}]{{geometry}}
\usepackage{{enumitem}}
\usepackage{{amssymb}}
\usepackage{{fancyvrb}}
\usepackage[dvipsnames]{{xcolor}}
\usepackage[colorlinks=true,linkcolor=blue,urlcolor=blue,bookmarks=false]{{hyperref}}
\setmainfont{{{font}}}
\newfontfamily\fbfont{{DejaVu Sans}}
\setlength{{\parindent}}{{0pt}}
\setlength{{\parskip}}{{0.6em}}
{color_defs}{heading_cmds}\begin{{document}}
\fontsize{{{size}pt}}{{{leading:.2}pt}}\selectfont
{body}\end{{document}}
"#,
        margin = pdf.margin,
        font = pdf.font,
        size = pdf.font_size,
        leading = leading,
        color_defs = color_defs,
        heading_cmds = heading_cmds,
        body = body,
    )
}

// ── Preamble builder ──────────────────────────────────────────────────────────

/// Build `\definecolor` and `\newcommand{\hone}`–`\newcommand{\hsix}` from the
/// user's configured heading colors and glyphs. Unicode chars are used directly;
/// missing codepoints fall through to the FallbackFonts set on \setmainfont.
fn build_heading_preamble(ui: &UiConfig, bw: bool) -> (String, String) {
    let cmd_names = ["hone", "htwo", "hthree", "hfour", "hfive", "hsix"];
    let indents   = ["0em", "1.5em", "3em", "4.5em", "6em", "7.5em"];

    let mut color_defs   = String::new();
    let mut heading_cmds = String::new();

    for level in 1u8..=6 {
        let i = (level - 1) as usize;
        let (r, g, b) = if bw {
            let (cr, cg, cb) = ui.heading_color_rgb(level);
            let luma = (0.299 * cr as f32 + 0.587 * cg as f32 + 0.114 * cb as f32) as u8;
            (luma, luma, luma)
        } else {
            ui.heading_color_rgb(level)
        };
        let hex    = format!("{r:02X}{g:02X}{b:02X}");
        let color  = format!("notah{level}");
        let cmd    = cmd_names[i];
        let indent = indents[i];
        let glyph  = ui.heading_glyph_char(level);

        color_defs.push_str(&format!("\\definecolor{{{color}}}{{HTML}}{{{hex}}}\n"));

        // \newcommand{\hone}[1]{\noindent\hspace{0em}{\normalsize\bfseries\textcolor{notah1}{◉ #1}}\par}
        let mut def = String::new();
        def.push_str(&format!("\\newcommand{{\\{cmd}}}[1]{{"));
        def.push_str(&format!("\\noindent\\hspace{{{indent}}}{{"));
        def.push_str(&format!("\\normalsize\\bfseries\\textcolor{{{color}}}{{"));
        def.push_str("{\\fbfont ");
        def.push(glyph);
        def.push_str("} #1}}\\par}\n");
        heading_cmds.push_str(&def);
    }

    (color_defs, heading_cmds)
}

// ── Body translator ───────────────────────────────────────────────────────────

fn translate_body(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut in_todo_list = false;
    let mut in_code = false;

    for &line in lines {
        // When inside a code block, only look for the closing fence.
        if in_code {
            match parse_line(line) {
                BlockToken::CodeFenceClose => {
                    out.push_str("\\end{Verbatim}\n");
                    in_code = false;
                }
                _ => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            continue;
        }

        let token = parse_line(line);

        // Close the todo list before any non-todo token.
        let is_todo = matches!(
            &token,
            BlockToken::TodoOpen(_) | BlockToken::TodoDone(_) | BlockToken::TodoPending(_)
        );
        if in_todo_list && !is_todo {
            out.push_str("\\end{itemize}\n");
            in_todo_list = false;
        }

        match token {
            BlockToken::Heading { level, text } => {
                let cmd = ["hone", "htwo", "hthree", "hfour", "hfive", "hsix"]
                    [(level - 1).min(5) as usize];
                out.push_str(&format!("\\{cmd}{{{}}}\n", inline_to_latex(&text)));
            }

            BlockToken::TodoOpen(text) => {
                if !in_todo_list {
                    out.push_str("\\begin{itemize}[leftmargin=1.5em,label={}]\n");
                    in_todo_list = true;
                }
                out.push_str(&format!(
                    "\\item[\\textbullet~( )] {}\n",
                    inline_to_latex(&text)
                ));
            }
            BlockToken::TodoDone(text) => {
                if !in_todo_list {
                    out.push_str("\\begin{itemize}[leftmargin=1.5em,label={}]\n");
                    in_todo_list = true;
                }
                out.push_str(&format!(
                    "\\item[\\textcolor{{gray}}{{\\textbullet~(x)}}] \\textcolor{{gray}}{{{}}}\n",
                    inline_to_latex(&text)
                ));
            }
            BlockToken::TodoPending(text) => {
                if !in_todo_list {
                    out.push_str("\\begin{itemize}[leftmargin=1.5em,label={}]\n");
                    in_todo_list = true;
                }
                out.push_str(&format!(
                    "\\item[\\textbullet~(-)] {}\n",
                    inline_to_latex(&text)
                ));
            }

            BlockToken::CodeFenceOpen { .. } => {
                out.push_str("\\begin{Verbatim}[frame=single,fontsize=\\small]\n");
                in_code = true;
            }
            BlockToken::CodeFenceClose => {}

            BlockToken::Blank => {
                out.push('\n');
            }

            BlockToken::PlainText(text) => {
                out.push_str(&inline_to_latex(&text));
                out.push_str("\n\n");
            }
        }
    }

    if in_todo_list {
        out.push_str("\\end{itemize}\n");
    }
    if in_code {
        out.push_str("\\end{Verbatim}\n");
    }

    out
}

// ── Inline translator ─────────────────────────────────────────────────────────

fn inline_to_latex(text: &str) -> String {
    let mut out = String::new();
    for tok in parse_inline(text) {
        match tok {
            InlineToken::Plain(s) => out.push_str(&escape_latex(&s)),
            InlineToken::Bold(s) => {
                out.push_str("\\textbf{");
                out.push_str(&escape_latex(&s));
                out.push('}');
            }
            InlineToken::Italic(s) => {
                out.push_str("\\textit{");
                out.push_str(&escape_latex(&s));
                out.push('}');
            }
            InlineToken::Link { path, label } => {
                let display = label.as_deref().unwrap_or(&path).to_string();
                if path.starts_with("http://") || path.starts_with("https://") {
                    out.push_str(&format!(
                        "\\href{{{}}}{{{}}}",
                        path.replace('%', "\\%"),
                        escape_latex(&display)
                    ));
                } else {
                    out.push_str(&escape_latex(&display));
                }
            }
        }
    }
    out
}

fn escape_latex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&'  => out.push_str("\\&"),
            '%'  => out.push_str("\\%"),
            '$'  => out.push_str("\\$"),
            '#'  => out.push_str("\\#"),
            '_'  => out.push_str("\\_"),
            '{'  => out.push_str("\\{"),
            '}'  => out.push_str("\\}"),
            '~'  => out.push_str("\\textasciitilde{}"),
            '^'  => out.push_str("\\textasciicircum{}"),
            '\\' => out.push_str("\\textbackslash{}"),
            c    => out.push(c),
        }
    }
    out
}
