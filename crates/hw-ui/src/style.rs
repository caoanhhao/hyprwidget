//! SCSS (grass) → `GtkCssProvider`, the generated `_hypr.scss`, and inline
//! per-widget CSS.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use gtk4::{gdk, prelude::*};
use serde_json::Value;

/// Built-in stylesheet; uses the variables of `_hypr.scss` so widgets match
/// Hyprland before the user writes any CSS.
pub const DEFAULT_SCSS: &str = include_str!("default.scss");

/// Compiles `entry` (`.scss`/`.sass` through grass, `.css` as-is) with
/// `load_paths` searched for `@use`/`@import`. A missing entry yields "".
pub fn compile_file(entry: &Path, load_paths: &[&Path]) -> Result<String> {
    if !entry.exists() {
        return Ok(String::new());
    }
    if entry.extension().is_some_and(|e| e == "css") {
        return std::fs::read_to_string(entry)
            .with_context(|| format!("reading {}", entry.display()));
    }
    let opts = grass::Options::default().load_paths(load_paths);
    grass::from_path(entry, &opts).map_err(|e| anyhow::anyhow!("{}: {e}", entry.display()))
}

/// Compiles SCSS source text.
pub fn compile_str(scss: &str, load_paths: &[&Path]) -> Result<String> {
    let opts = grass::Options::default().load_paths(load_paths);
    grass::from_string(scss.to_owned(), &opts).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Owns the CSS providers of the app.
///
/// * `base`: CSS custom properties + [`DEFAULT_SCSS`]
/// * `user`: the user's stylesheet
/// * `inline`: `css = "…"` / `theme = {…}` of individual widgets
pub struct Styles {
    gen_dir: PathBuf,
    entry: RefCell<PathBuf>,
    base: gtk4::CssProvider,
    user: gtk4::CssProvider,
    inline: gtk4::CssProvider,
    inline_rules: RefCell<BTreeMap<u64, String>>,
    next_inline: Cell<u64>,
}

impl Styles {
    /// `gen_dir` receives `_hypr.scss`; `entry` is the user's stylesheet.
    pub fn new(gen_dir: PathBuf, entry: PathBuf) -> Result<Rc<Self>> {
        std::fs::create_dir_all(&gen_dir)
            .with_context(|| format!("creating {}", gen_dir.display()))?;
        let display = gdk::Display::default().context("no display")?;
        let make = |prio: u32, what: &'static str| {
            let p = gtk4::CssProvider::new();
            p.connect_parsing_error(move |_, section, err| {
                tracing::warn!("{what} CSS {}: {err}", section.to_str());
            });
            gtk4::style_context_add_provider_for_display(&display, &p, prio);
            p
        };
        Ok(Rc::new(Self {
            // Above ~/.config/gtk-4.0/gtk.css (USER), which themes such as
            // Orchis use to paint every `.background` window.
            base: make(gtk4::STYLE_PROVIDER_PRIORITY_USER + 1, "default"),
            user: make(gtk4::STYLE_PROVIDER_PRIORITY_USER + 2, "user"),
            inline: make(gtk4::STYLE_PROVIDER_PRIORITY_USER + 3, "inline"),
            gen_dir,
            entry: RefCell::new(entry),
            inline_rules: RefCell::new(BTreeMap::new()),
            next_inline: Cell::new(0),
        }))
    }

    fn style_dir(&self) -> PathBuf {
        self.entry
            .borrow()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    pub fn set_entry(&self, entry: PathBuf) {
        *self.entry.borrow_mut() = entry;
    }

    /// Writes `_hypr.scss` for the merged option tree and rebuilds the
    /// default stylesheet.
    pub fn set_options(&self, tree: &Value) -> Result<()> {
        let scss = hw_core::options::to_scss(tree);
        let path = self.gen_dir.join("_hypr.scss");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(scss.as_str()) {
            std::fs::write(&path, &scss).with_context(|| format!("writing {}", path.display()))?;
        }
        let css = compile_str(DEFAULT_SCSS, &[&self.gen_dir])?;
        let vars = hw_core::options::to_css_vars(tree);
        self.base.load_from_string(&format!("{vars}\n{css}"));
        Ok(())
    }

    /// Recompiles the user's stylesheet. On error the previous CSS stays.
    pub fn reload_user(&self) -> Result<()> {
        let entry = self.entry.borrow().clone();
        let style_dir = self.style_dir();
        let css = compile_file(&entry, &[&style_dir, &self.gen_dir])?;
        self.user.load_from_string(&css);
        tracing::info!(file = %entry.display(), "stylesheet loaded");
        Ok(())
    }

    /// Applies `declarations` to `widget` only; returns a handle that removes
    /// them again (call it when the widget goes away or the CSS changes).
    pub fn set_inline(self: &Rc<Self>, widget: &gtk4::Widget, declarations: &str) -> InlineCss {
        let id = self.next_inline.get();
        self.next_inline.set(id + 1);
        let class = format!("hw-inline-{id}");
        widget.add_css_class(&class);
        self.inline_rules
            .borrow_mut()
            .insert(id, format!(".{class} {{ {declarations} }}"));
        self.rebuild_inline();
        InlineCss {
            styles: Rc::downgrade(self),
            id,
        }
    }

    fn rebuild_inline(&self) {
        let css: String = self
            .inline_rules
            .borrow()
            .values()
            .map(|r| format!("{r}\n"))
            .collect();
        self.inline.load_from_string(&css);
    }
}

/// Removes an inline rule on drop.
pub struct InlineCss {
    styles: std::rc::Weak<Styles>,
    id: u64,
}

impl Drop for InlineCss {
    fn drop(&mut self) {
        if let Some(s) = self.styles.upgrade() {
            s.inline_rules.borrow_mut().remove(&self.id);
            s.rebuild_inline();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scss_compiles_with_generated_vars() {
        let dir = std::env::temp_dir().join(format!("hw-style-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("_hypr.scss"),
            hw_core::options::to_scss(&hw_core::options::defaults()),
        )
        .unwrap();
        let css = compile_str(DEFAULT_SCSS, &[&dir]).unwrap();
        assert!(css.contains("border-radius: 0px"), "{css}");
    }
}
