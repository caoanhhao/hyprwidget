//! `hw.set_option(name, value [, target])`: what ControlPanel/UserPanel do
//! when an option is changed (PLAN.md §3.7). The option is written to
//! `config/custom/<name>.lua`, in the same format AGS writes, and applied
//! live:
//!
//! * Hyprland options → `<hypr_custom_dir>/<name>.lua` with
//!   `hl.config({ decoration = { rounding = 16 } })`, applied with
//!   `hyprctl eval` of that same line (Hyprland 0.57 has no `keyword`), then
//!   the inherited options are re-read.
//! * widget options → `<config dir>/config/custom/<name>.lua` with
//!   `hw.config({ … })` (loaded by `require_custom_dir()`), merged into the
//!   user option layer right away.
//!
//! `target` is `"hypr"`, `"widget"` or (default) `"auto"`: Hyprland's
//! option if Hyprland knows the name. Returns the target used.

use std::path::{Path, PathBuf};

use hw_core::{hyprland, options};
use mlua::{Lua, Result, Table, Value};
use serde_json::Value as Json;

use crate::convert::{LuaResultExt, lua_to_json};
use crate::runtime::{RtState, apply_options};

pub fn install(lua: &Lua, hw: &Table) -> Result<()> {
    hw.set(
        "set_option",
        lua.create_function(
            |lua, (name, value, target): (String, Value, Option<String>)| {
                set_option(lua, &name, value, target.as_deref().unwrap_or("auto"))
                    .map_err(|e| mlua::Error::runtime(format!("hw.set_option({name}): {e:#}")))
            },
        )?,
    )?;
    Ok(())
}

fn set_option(lua: &Lua, name: &str, value: Value, target: &str) -> anyhow::Result<String> {
    let st = RtState::get(lua).anyhow()?;
    let path = options::name_to_path(name);
    anyhow::ensure!(
        !path.is_empty() && path.iter().all(|s| valid_segment(s)),
        "invalid option name"
    );
    let name = options::path_to_name(&path);
    let value = lua_to_json(lua, value).anyhow()?;
    anyhow::ensure!(
        matches!(
            value,
            Json::Bool(_) | Json::Number(_) | Json::String(_) | Json::Array(_)
        ),
        "value must be a boolean, number, string or list"
    );
    let hypr = match target {
        "hypr" => true,
        "widget" => false,
        "auto" => is_hypr_option(&name),
        other => anyhow::bail!("target must be \"hypr\", \"widget\" or \"auto\", got {other:?}"),
    };
    if hypr {
        let line = lua_config("hl", &path, &value);
        let dir = st.core.app_cfg.borrow().hypr_custom_dir.clone();
        write_custom(&dir, &name, &line)?;
        if hyprland::available() {
            hyprland::eval(&line)?;
            st.core.refresh_hypr();
        }
        Ok("hypr".into())
    } else {
        let line = lua_config("hw", &path, &value);
        write_custom(&st.core.dir.root.join("config/custom"), &name, &line)?;
        let v = options::normalize(&name, &value).map_err(anyhow::Error::msg)?;
        options::insert(&mut st.user_opts.borrow_mut(), &path, v);
        apply_options(lua, &st);
        Ok("widget".into())
    }
}

fn valid_segment(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Hyprland's own option? Asks the running Hyprland, else the list of
/// options hyprwidget inherits.
fn is_hypr_option(name: &str) -> bool {
    if hyprland::available()
        && let Ok(r) = hyprland::get_options(&[name])
    {
        return r.into_iter().next().flatten().is_some();
    }
    options::inherited(name).is_some()
}

fn write_custom(dir: &Path, name: &str, line: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join(format!("{name}.lua"));
    std::fs::write(&file, format!("{line}\n"))
        .map_err(|e| anyhow::anyhow!("writing {}: {e}", file.display()))?;
    tracing::info!("option {name} written to {}", file.display());
    Ok(file)
}

/// `hl.config({ decoration = { rounding = 16 } })` (AGS' `buildLuaConfig`).
pub fn lua_config(func: &str, path: &[String], value: &Json) -> String {
    let mut expr = lua_value(value);
    for seg in path.iter().rev() {
        expr = format!("{{ {} = {expr} }}", lua_key(seg));
    }
    format!("{func}.config({expr})")
}

fn lua_key(k: &str) -> String {
    if !k.is_empty()
        && !k.starts_with(|c: char| c.is_ascii_digit())
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        k.to_owned()
    } else {
        format!("[\"{k}\"]")
    }
}

fn lua_value(v: &Json) -> String {
    match v {
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => {
                format!("{}", crate::convert::whole_f64_to_i64(f))
            }
            _ => n.to_string(),
        },
        Json::Array(a) => format!(
            "{{ {} }}",
            a.iter().map(lua_value).collect::<Vec<_>>().join(", ")
        ),
        Json::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        _ => "nil".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(name: &str) -> Vec<String> {
        options::name_to_path(name)
    }

    #[test]
    fn same_format_as_ags() {
        assert_eq!(
            lua_config("hl", &p("decoration:rounding"), &json!(16)),
            "hl.config({ decoration = { rounding = 16 } })"
        );
        assert_eq!(
            lua_config("hl", &p("decoration:blur:enabled"), &json!(true)),
            "hl.config({ decoration = { blur = { enabled = true } } })"
        );
        assert_eq!(
            lua_config("hl", &p("decoration:active_opacity"), &json!(0.9)),
            "hl.config({ decoration = { active_opacity = 0.9 } })"
        );
        assert_eq!(
            lua_config("hw", &p("blur:ignore_alpha"), &json!(16.0)),
            "hw.config({ blur = { ignore_alpha = 16 } })"
        );
        assert_eq!(
            lua_config("hl", &p("general:col.active_border"), &json!("rgb(80\"80)")),
            "hl.config({ general = { col = { active_border = \"rgb(80\\\"80)\" } } })"
        );
        assert_eq!(
            lua_config("hw", &p("a:1x"), &json!(["bottom", "left"])),
            "hw.config({ a = { [\"1x\"] = { \"bottom\", \"left\" } } })"
        );
    }
}
