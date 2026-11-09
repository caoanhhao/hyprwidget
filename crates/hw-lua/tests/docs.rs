//! Keeps `docs/` (English), `docs/vi/` (Vietnamese) and the LuaLS stubs in
//! `types/` in sync with the code: every widget kind, event, inherited
//! option, native service and service prop must be documented and typed.

use std::path::PathBuf;

use hw_core::options::INHERITED;
use hw_ui::widgets::{EVENTS, Kind};

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
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

/// Documentation directories, one per language.
const DOC_DIRS: [&str; 2] = ["docs", "docs/vi"];

#[test]
fn docs_cover_api() {
    let mut missing = Vec::new();
    for dir in DOC_DIRS {
        check_docs(dir, &mut missing);
    }
    check_stubs(&mut missing);
    assert!(
        missing.is_empty(),
        "docs/stubs out of date:\n  {}",
        missing.join("\n  ")
    );
}

fn check_docs(dir: &str, missing: &mut Vec<String>) {
    let widgets_md = read(&format!("{dir}/widgets.md"));
    let options_md = read(&format!("{dir}/options.md"));
    let services_md = read(&format!("{dir}/services.md"));

    for k in Kind::ALL {
        let n = k.name();
        if !widgets_md.contains(&format!("| `{n}` |")) {
            missing.push(format!("{dir}/widgets.md: widget table row for `{n}`"));
        }
    }
    for ev in EVENTS {
        if !widgets_md.contains(&format!("| `{ev}`")) {
            missing.push(format!("{dir}/widgets.md: event `{ev}`"));
        }
    }
    for (name, ..) in INHERITED {
        if !options_md.contains(&format!("| `{name}` |")) {
            missing.push(format!("{dir}/options.md: option `{name}`"));
        }
    }
    for svc in hw_services::NAMES {
        let props = hw_services::props(svc).expect("props of every native service");
        let Some(md) = section(&services_md, &format!("## {svc}\n"), "## ") else {
            missing.push(format!("{dir}/services.md: section `## {svc}`"));
            continue;
        };
        for p in props {
            if !md.contains(&format!("`{p}`")) {
                missing.push(format!("{dir}/services.md: {svc}.{p}"));
            }
        }
    }
}

fn check_stubs(missing: &mut Vec<String>) {
    let widgets_lua = read("types/widgets.lua");
    let options_lua = read("types/options.lua");
    let services_lua = read("types/services.lua");

    for k in Kind::ALL {
        let n = k.name();
        if !widgets_lua.contains(&format!("---@field {n} fun(props: hw.{n}Props")) {
            missing.push(format!("types/widgets.lua: hw.W field {n}"));
        }
        if !widgets_lua.contains(&format!("---@class hw.{n}Props")) {
            missing.push(format!("types/widgets.lua: class hw.{n}Props"));
        }
    }
    for ev in EVENTS {
        if !widgets_lua.contains(&format!("\"{ev}\"")) {
            missing.push(format!("types/widgets.lua: hw.EventName {ev}"));
        }
    }
    for (name, ..) in INHERITED {
        if !options_lua.contains(&format!("---| \"{name}\"")) {
            missing.push(format!("types/options.lua: hw.OptionName {name}"));
        }
    }
    for svc in hw_services::NAMES {
        let props = hw_services::props(svc).expect("props of every native service");
        let Some(lua) = section(
            &services_lua,
            &format!("---@class hw.services.{svc}:"),
            "---@class",
        ) else {
            missing.push(format!("types/services.lua: class hw.services.{svc}"));
            continue;
        };
        for p in props {
            if !lua.contains(&format!("---@field {p} hw.Signal")) {
                missing.push(format!("types/services.lua: {svc}.{p}"));
            }
        }
    }
}
