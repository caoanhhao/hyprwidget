//! Keeps `docs/` and the LuaLS stubs in `types/` in sync with the code:
//! every widget kind, event, inherited option, native service and service
//! prop must be documented and typed.

use std::path::PathBuf;

use hw_core::options::INHERITED;
use hw_ui::widgets::{EVENTS, Kind};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Text from the line starting with `start` up to the next line starting
/// with `next` (or the end).
fn section<'a>(text: &'a str, start: &str, next: &str) -> Option<&'a str> {
    let i = text
        .match_indices(start)
        .find(|(i, _)| *i == 0 || text.as_bytes()[i - 1] == b'\n')?
        .0;
    let rest = &text[i + start.len()..];
    let end = rest.find(&format!("\n{next}")).unwrap_or(rest.len());
    Some(&rest[..end])
}

#[test]
fn docs_cover_api() {
    let widgets_md = read("docs/widgets.md");
    let widgets_lua = read("types/widgets.lua");
    let options_md = read("docs/options.md");
    let options_lua = read("types/options.lua");
    let services_md = read("docs/services.md");
    let services_lua = read("types/services.lua");
    let mut missing = Vec::new();

    for k in Kind::ALL {
        let n = k.name();
        if !widgets_md.contains(&format!("| `{n}` |")) {
            missing.push(format!("docs/widgets.md: widget table row for `{n}`"));
        }
        if !widgets_lua.contains(&format!("---@field {n} fun(props: hw.{n}Props")) {
            missing.push(format!("types/widgets.lua: hw.W field {n}"));
        }
        if !widgets_lua.contains(&format!("---@class hw.{n}Props")) {
            missing.push(format!("types/widgets.lua: class hw.{n}Props"));
        }
    }
    for ev in EVENTS {
        if !widgets_md.contains(&format!("| `{ev}`")) {
            missing.push(format!("docs/widgets.md: event `{ev}`"));
        }
        if !widgets_lua.contains(&format!("\"{ev}\"")) {
            missing.push(format!("types/widgets.lua: hw.EventName {ev}"));
        }
    }
    for (name, ..) in INHERITED {
        if !options_md.contains(&format!("| `{name}` |")) {
            missing.push(format!("docs/options.md: option `{name}`"));
        }
        if !options_lua.contains(&format!("---| \"{name}\"")) {
            missing.push(format!("types/options.lua: hw.OptionName {name}"));
        }
    }
    for svc in hw_services::NAMES {
        let props = hw_services::props(svc).expect("props of every native service");
        let md = section(&services_md, &format!("## {svc}\n"), "## ");
        let lua = section(
            &services_lua,
            &format!("---@class hw.services.{svc}:"),
            "---@class",
        );
        if md.is_none() {
            missing.push(format!("docs/services.md: section `## {svc}`"));
        }
        if lua.is_none() {
            missing.push(format!("types/services.lua: class hw.services.{svc}"));
        }
        for p in props {
            if md.is_some_and(|s| !s.contains(&format!("`{p}`"))) {
                missing.push(format!("docs/services.md: {svc}.{p}"));
            }
            if lua.is_some_and(|s| !s.contains(&format!("---@field {p} hw.Signal"))) {
                missing.push(format!("types/services.lua: {svc}.{p}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "docs/stubs out of date:\n  {}",
        missing.join("\n  ")
    );
}
