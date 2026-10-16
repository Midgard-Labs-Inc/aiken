//! Render a project's documentation as MDX pages for a [Nimbus](https://nimbus-docs.com) site:
//! one page per module, laid out after the module paths so that the site's sidebar groups
//! them, with `{#name}` heading ids so definitions can be linked to.
//!
//! Links between modules are root-relative, from the URL path the pages are mounted at
//! (`base`), and use directory-style URLs (Astro's default).

use super::{
    DocFile, Interspersed, ModuleMembers, dedent, is_documented, source_links::SourceLinker,
};
use crate::{config::ProjectConfig, module::CheckedModule};
use itertools::Itertools;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use regex::{Captures, Regex};
use std::{
    collections::BTreeSet,
    ops::Range,
    path::{Path, PathBuf},
};

pub fn generate_all(
    root: &Path,
    config: &ProjectConfig,
    modules: Vec<&CheckedModule>,
    base: &str,
) -> Vec<DocFile> {
    let modules = modules
        .into_iter()
        .filter(|module| is_documented(module))
        .sorted_by(|a, b| a.name.cmp(&b.name))
        .collect_vec();

    let names: BTreeSet<&str> = modules.iter().map(|module| module.name.as_str()).collect();

    let mut files = vec![generate_index(root, config, &names, base)];

    for module in &modules {
        files.push(generate_module(root, config, module, &names, base));
    }

    files
}

/// The project's README, as the landing page.
fn generate_index(
    root: &Path,
    config: &ProjectConfig,
    names: &BTreeSet<&str>,
    base: &str,
) -> DocFile {
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap_or_default();

    // The page title already is the page's heading.
    let readme = match readme.split_once('\n') {
        Some((first, rest)) if first.starts_with("# ") => rest.to_string(),
        _ => readme,
    };

    let mut page = frontmatter(&config.name.to_string(), None, false);
    push_markdown(&mut page, &readme, &Links::new("", names, base));

    DocFile {
        path: PathBuf::from("index.mdx"),
        content: page,
    }
}

fn generate_module(
    root: &Path,
    config: &ProjectConfig,
    module: &CheckedModule,
    names: &BTreeSet<&str>,
    base: &str,
) -> DocFile {
    let source_linker = SourceLinker::new(root, config, module);
    let ModuleMembers {
        functions,
        types,
        constants,
    } = ModuleMembers::new(module, &source_linker);

    let links = Links::new(&module.name, names, base);

    let path = page_path(&module.name, names);
    let label = module.name.rsplit('/').next().unwrap_or(&module.name);
    let mut page = frontmatter(&module.name, Some(label), path.ends_with("index.mdx"));

    push_markdown(&mut page, &module.ast.docs.iter().join("\n"), &links);

    if !types.is_empty() {
        page.push_str("## Types\n\n");

        for type_info in types {
            let parameters = if type_info.parameters.is_empty() {
                String::new()
            } else {
                format!("<{}>", type_info.parameters.join(", "))
            };

            page.push_str(&format!(
                "### `{}{parameters}` {{#{}}}\n\n",
                type_info.name, type_info.name
            ));
            push_markdown(&mut page, &type_info.raw_documentation, &links);

            if !type_info.constructors.is_empty() {
                page.push_str("#### Constructors\n\n");
                for constructor in type_info.constructors {
                    push_code(&mut page, &constructor.definition);
                    push_markdown(&mut page, &constructor.markdown, &links);
                }
            } else if !type_info.opaque {
                page.push_str("#### Alias\n\n");
                push_code(&mut page, &type_info.definition);
            }
        }
    }

    if !constants.is_empty() {
        page.push_str("## Constants\n\n");

        for constant in constants {
            page.push_str(&format!(
                "### `{}` {{#{}}}\n\n",
                constant.name, constant.name
            ));
            push_code(&mut page, &constant.definition);
            push_markdown(&mut page, &constant.raw_documentation, &links);
        }
    }

    if functions
        .iter()
        .any(|item| matches!(item, Interspersed::Function(..)))
    {
        page.push_str("## Functions\n\n");

        for item in functions {
            match item {
                Interspersed::Section(section) => {
                    let level = match section.heading {
                        1 | 2 => "###",
                        3 => "####",
                        _ => "#####",
                    };
                    page.push_str(&format!("{level} {}\n\n", escape_text(&section.title)));
                }
                Interspersed::Function(function) => {
                    page.push_str(&format!(
                        "### `{}` {{#{}}}\n\n",
                        function.name, function.name
                    ));
                    push_code(&mut page, &function.signature);
                    push_markdown(&mut page, &function.raw_documentation, &links);
                }
            }
        }
    }

    DocFile {
        path,
        content: page.trim_end().to_string() + "\n",
    }
}

/// A module with sub-modules is the landing page of their folder.
fn page_path(module: &str, names: &BTreeSet<&str>) -> PathBuf {
    if names
        .iter()
        .any(|other| other.starts_with(&format!("{module}/")))
    {
        PathBuf::from(module).join("index.mdx")
    } else {
        PathBuf::from(format!("{module}.mdx"))
    }
}

/// A module with sub-modules is the landing page of their sidebar group, which then takes its
/// label too.
fn frontmatter(title: &str, label: Option<&str>, landing_page: bool) -> String {
    let quote = |s: &str| serde_json::to_string(s).expect("serialising a string");

    let mut frontmatter = format!("---\ntitle: {}\n", quote(title));
    if let Some(label) = label {
        frontmatter.push_str(&format!("sidebar:\n  label: {}\n", quote(label)));
        if landing_page {
            frontmatter.push_str(&format!("  group:\n    label: {}\n", quote(label)));
        }
    }
    frontmatter.push_str("---\n\n");
    frontmatter
}

fn push_code(page: &mut String, code: &str) {
    page.push_str(&format!("```aiken\n{code}\n```\n\n"));
}

fn push_markdown(page: &mut String, markdown: &str, links: &Links) {
    let markdown = dedent(markdown);
    let markdown = markdown.trim();
    if !markdown.is_empty() {
        page.push_str(&escape_mdx(&links.rewrite(markdown)));

        // Each doc comment used to be rendered on its own, which forgave a missing closing
        // fence; on a page, it would swallow everything that follows.
        let fences = markdown
            .lines()
            .filter(|line| line.trim_start().starts_with("```"))
            .count();
        if fences % 2 == 1 {
            page.push_str("\n```");
        }

        page.push_str("\n\n");
    }
}

/// Rewrites links to other modules' HTML documentation (e.g. `../dict.html#Dict`, relative to
/// the module's location) into links to their pages.
struct Links<'a> {
    module: &'a str,
    names: &'a BTreeSet<&'a str>,
    base: &'a str,
    pattern: Regex,
}

impl<'a> Links<'a> {
    fn new(module: &'a str, names: &'a BTreeSet<&'a str>, base: &'a str) -> Self {
        Links {
            module,
            names,
            base: base.trim_end_matches('/'),
            pattern: Regex::new(r"\]\(((?:\.\.?/)?[\w./-]+?)\.html(#[^)\s]*)?\)").unwrap(),
        }
    }

    fn rewrite(&self, markdown: &str) -> String {
        self.pattern
            .replace_all(markdown, |caps: &Captures| {
                let anchor = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
                match self.resolve(&caps[1]) {
                    Some(target) if target == self.module && !anchor.is_empty() => {
                        format!("]({anchor})")
                    }
                    Some(target) => format!("]({}/{target}/{anchor})", self.base),
                    None => caps[0].to_string(),
                }
            })
            .to_string()
    }

    /// The module a link points to, relative to the linking module's location.
    fn resolve(&self, link: &str) -> Option<String> {
        let mut segments = self
            .module
            .split('/')
            .filter(|s| !s.is_empty())
            .collect_vec();
        segments.pop();

        for segment in link.split('/') {
            match segment {
                "." | "" => {}
                ".." => {
                    segments.pop()?;
                }
                segment => segments.push(segment),
            }
        }

        let target = segments.join("/");
        self.names.contains(target.as_str()).then_some(target)
    }
}

/// Make Markdown safe for MDX: escape `{`, `}` and `<` in text (MDX would otherwise read them
/// as expressions and tags), and turn autolinks (`<https://…>`), which MDX doesn't support,
/// into regular links. Code, math and inline HTML are left untouched.
fn escape_mdx(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH;

    let mut edits: Vec<(Range<usize>, String)> = vec![];
    let mut in_code_block = false;
    let mut autolink_end = 0;

    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(..)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Start(Tag::Link {
                link_type: link_type @ (LinkType::Autolink | LinkType::Email),
                dest_url,
                ..
            }) => {
                let href = if link_type == LinkType::Email {
                    format!("mailto:{dest_url}")
                } else {
                    dest_url.to_string()
                };
                edits.push((range.clone(), format!("[{dest_url}]({href})")));
                autolink_end = range.end;
            }
            Event::Text(..) if !in_code_block && range.start >= autolink_end => {
                let text = &markdown[range.clone()];
                if text.contains(['{', '}', '<']) {
                    edits.push((range, escape_text(text)));
                }
            }
            _ => {}
        }
    }

    edits.sort_by_key(|(range, _)| range.start);

    let mut escaped = String::with_capacity(markdown.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        if range.start < cursor {
            continue;
        }
        escaped.push_str(&markdown[cursor..range.start]);
        escaped.push_str(&replacement);
        cursor = range.end;
    }
    escaped.push_str(&markdown[cursor..]);
    escaped
}

fn escape_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    let mut previous = None;
    for c in text.chars() {
        if matches!(c, '{' | '}' | '<') && previous != Some('\\') {
            escaped.push('\\');
        }
        escaped.push(c);
        previous = Some(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text_but_not_code() {
        assert_eq!(
            escape_mdx("`Spend` < `Mint` and {x}\n\n```aiken\nfn f() { a < b }\n```"),
            "`Spend` \\< `Mint` and \\{x\\}\n\n```aiken\nfn f() { a < b }\n```"
        );
    }

    #[test]
    fn keeps_math_and_html() {
        assert_eq!(
            escape_mdx("an interval $[start_{bid}, end_{bid})$<br/>ok"),
            "an interval $[start_{bid}, end_{bid})$<br/>ok"
        );
    }

    #[test]
    fn dedents_doc_comments() {
        assert_eq!(
            dedent(" Add an element.\n\n ```aiken\n   list.push(xs, 1)\n ```"),
            "Add an element.\n\n```aiken\n  list.push(xs, 1)\n```"
        );
    }

    #[test]
    fn rewrites_autolinks() {
        assert_eq!(
            escape_mdx("see <https://example.com/a#b>."),
            "see [https://example.com/a#b](https://example.com/a#b)."
        );
    }

    #[test]
    fn rewrites_module_links() {
        let names = BTreeSet::from([
            "aiken/collection/dict",
            "aiken/collection/list",
            "cardano/address",
            "cardano/transaction",
            "cardano/transaction/script_purpose",
        ]);

        let links = Links::new("aiken/collection/list", &names, "/stdlib/");
        assert_eq!(
            links.rewrite("[Dict](./dict.html#Dict) and [Foo](./foo.html)"),
            "[Dict](/stdlib/aiken/collection/dict/#Dict) and [Foo](./foo.html)"
        );

        let links = Links::new("cardano/transaction/script_purpose", &names, "/stdlib");
        assert_eq!(
            links.rewrite("[ScriptPurpose](../transaction.html#ScriptPurpose)"),
            "[ScriptPurpose](/stdlib/cardano/transaction/#ScriptPurpose)"
        );

        let links = Links::new("cardano/transaction", &names, "/stdlib");
        assert_eq!(
            links.rewrite("[ScriptPurpose](./transaction.html#ScriptPurpose)"),
            "[ScriptPurpose](#ScriptPurpose)"
        );

        let links = Links::new("", &names, "/");
        assert_eq!(
            links.rewrite("[list](aiken/collection/list.html)"),
            "[list](/aiken/collection/list/)"
        );
    }
}
