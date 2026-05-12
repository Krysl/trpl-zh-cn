use html_escape::encode_double_quoted_attribute;
use regex::{Captures, Regex};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::process;
use syntect::html::{css_for_theme_with_class_style, ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use typst_library::text::RAW_THEME;

const GENERATED_CSS_PATH: &str = "theme/epub-syntect.css";

fn sanitize_markdown(text: &str, inline_break: &Regex, block_break: &Regex) -> String {
    let text = inline_break.replace_all(text, "$1\n>");
    block_break.replace_all(&text, ">" as &str).into_owned()
}

fn extract_language<'a>(info: &'a str, lang_aliases: &HashMap<&str, &str>) -> (String, String) {
    let normalized = info.trim();
    if normalized.is_empty() {
        return (String::new(), String::new());
    }

    let first = normalized.split_whitespace().next().unwrap_or_default();
    let mut base = first.split(',').next().unwrap_or_default().trim().to_lowercase();
    if base.starts_with('{') && base.ends_with('}') && base.len() >= 2 {
        base = base[1..base.len() - 1].trim().to_string();
    }

    let language = lang_aliases
        .get(base.as_str())
        .map(|s| (*s).to_string())
        .unwrap_or(base);

    (first.to_string(), language)
}

fn filter_rust_hidden_lines(code: &str) -> String {
    code.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_syntax<'a>(ps: &'a SyntaxSet, language: &str) -> Option<&'a SyntaxReference> {
    ps.find_syntax_by_token(language)
        .or_else(|| ps.find_syntax_by_extension(language))
}

fn highlight_code(ps: &SyntaxSet, syntax: &SyntaxReference, code: &str) -> Result<String, String> {
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, ps, ClassStyle::Spaced);

    for line in LinesWithEndings::from(code) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .map_err(|e| format!("failed to highlight line: {e}"))?;
    }

    Ok(generator.finalize())
}

fn highlight_fenced_blocks(
    text: &str,
    fenced_block: &Regex,
    ps: &SyntaxSet,
    lang_aliases: &HashMap<&str, &str>,
) -> String {
    fenced_block
        .replace_all(text, |caps: &Captures| {
            let info = caps.get(1).map(|m| m.as_str()).unwrap_or_default().trim();
            let code = caps.get(2).map(|m| m.as_str()).unwrap_or_default();

            let (original_class, language) = extract_language(info, lang_aliases);
            if language.is_empty() {
                return Cow::Owned(caps.get(0).map(|m| m.as_str()).unwrap_or_default().to_string());
            }

            let Some(syntax) = find_syntax(ps, &language) else {
                return Cow::Owned(caps.get(0).map(|m| m.as_str()).unwrap_or_default().to_string());
            };

            let code = if language == "rust" {
                filter_rust_hidden_lines(code)
            } else {
                code.to_string()
            };

            let Ok(highlighted) = highlight_code(ps, syntax, &code) else {
                return Cow::Owned(caps.get(0).map(|m| m.as_str()).unwrap_or_default().to_string());
            };

            let class_suffix = encode_double_quoted_attribute(&original_class);
            Cow::Owned(format!(
                "<pre><code class=\"language-{class_suffix}\">{highlighted}</code></pre>"
            ))
        })
        .into_owned()
}

fn process_markdown(
    text: &str,
    inline_break: &Regex,
    block_break: &Regex,
    fenced_block: &Regex,
    ps: &SyntaxSet,
    lang_aliases: &HashMap<&str, &str>,
) -> String {
    let sanitized = sanitize_markdown(text, inline_break, block_break);
    highlight_fenced_blocks(&sanitized, fenced_block, ps, lang_aliases)
}

fn walk_book(
    node: &mut Value,
    inline_break: &Regex,
    block_break: &Regex,
    fenced_block: &Regex,
    ps: &SyntaxSet,
    lang_aliases: &HashMap<&str, &str>,
) {
    match node {
        Value::Object(map) => {
            if let Some(Value::Object(chapter)) = map.get_mut("Chapter") {
                if let Some(Value::String(content)) = chapter.get_mut("content") {
                    let updated = process_markdown(
                        content,
                        inline_break,
                        block_break,
                        fenced_block,
                        ps,
                        lang_aliases,
                    );
                    *content = updated;
                }

                if let Some(sub_items) = chapter.get_mut("sub_items") {
                    walk_book(
                        sub_items,
                        inline_break,
                        block_break,
                        fenced_block,
                        ps,
                        lang_aliases,
                    );
                }
            } else {
                for value in map.values_mut() {
                    walk_book(
                        value,
                        inline_break,
                        block_break,
                        fenced_block,
                        ps,
                        lang_aliases,
                    );
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_book(
                    item,
                    inline_break,
                    block_break,
                    fenced_block,
                    ps,
                    lang_aliases,
                );
            }
        }
        _ => {}
    }
}

fn generate_syntect_css() -> Result<String, String> {
    let css = css_for_theme_with_class_style(&RAW_THEME, ClassStyle::Spaced)
        .map_err(|e| format!("failed to generate syntect css: {e}"))?;

    Ok(format!(
        "/* Auto-generated by scripts/epub_preprocess_rs. Do not edit manually. */\n{}\n",
        css
    ))
}

fn ensure_syntect_css_file() {
    let Ok(css) = generate_syntect_css() else {
        eprintln!("epub_preprocess_rs: failed to generate syntect css");
        return;
    };

    match fs::read_to_string(GENERATED_CSS_PATH) {
        Ok(existing) if existing == css => {}
        _ => {
            if let Err(err) = fs::write(GENERATED_CSS_PATH, css) {
                eprintln!(
                    "epub_preprocess_rs: failed to write {}: {}",
                    GENERATED_CSS_PATH, err
                );
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "supports" {
        let renderer = args.get(2).map(String::as_str).unwrap_or_default();
        process::exit((renderer == "epub") as i32 ^ 1);
    }

    if args.len() > 1 && args[1] == "generate-css" {
        ensure_syntect_css_file();
        return;
    }

    let inline_break = Regex::new(r"(?m)^(>\s*\[[^\]]+\]\([^)]+\))\s*<br\s*/?>\s*$")
        .expect("invalid inline_break regex");
    let block_break = Regex::new(r"(?m)^>\s*<br\s*/?>\s*$").expect("invalid block_break regex");
    let fenced_block =
        Regex::new(r"(?ms)^```([^\n`]*)\n(.*?)\n```[ \t]*$").expect("invalid fenced_block regex");

    let lang_aliases = HashMap::from([
        ("rs", "rust"),
        ("shell", "bash"),
        ("sh", "bash"),
        ("console", "bash"),
        ("ps1", "powershell"),
        ("pwsh", "powershell"),
        ("yml", "yaml"),
    ]);

    let ps = SyntaxSet::load_defaults_newlines();

    let mut raw = Vec::new();
    io::stdin()
        .read_to_end(&mut raw)
        .expect("failed to read stdin");

    let raw_str = String::from_utf8_lossy(&raw);
    let mut root: Value = serde_json::from_str(raw_str.trim_start_matches('\u{feff}'))
        .expect("failed to parse preprocessor json");

    let book = root
        .as_array_mut()
        .and_then(|arr| arr.get_mut(1))
        .expect("invalid preprocessor json format: expected [context, book]");

    walk_book(
        book,
        &inline_break,
        &block_break,
        &fenced_block,
        &ps,
        &lang_aliases,
    );

    let output = serde_json::to_vec(book).expect("failed to serialize book json");
    io::stdout().write_all(&output).expect("failed to write stdout");
}
