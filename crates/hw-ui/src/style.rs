//! SCSS (grass) → `GtkCssProvider`, the generated `_hypr.scss`, and inline
//! per-widget CSS.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use gtk4::{gdk, prelude::*};
use serde_json::Value;

/// Built-in stylesheet: resets every widget (`* { all: unset }`) so no GTK
/// theme applies, then styles them from the variables of `_hypr.scss` so
/// widgets match Hyprland before the user writes any CSS.
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

/// Compiles an extra stylesheet (a plugin's `style.scss`): `hypr` and
/// `hw-theme` from `gen_dir` are imported first, the file's own directory
/// is searched for `@import`.
pub fn compile_extra(entry: &Path, gen_dir: &Path) -> Result<String> {
    let text =
        std::fs::read_to_string(entry).with_context(|| format!("reading {}", entry.display()))?;
    let dir = entry.parent().unwrap_or(Path::new("."));
    // Same line, so error line numbers match the file.
    let src = format!("@import \"hypr\"; @import \"hw-theme\"; {text}");
    let opts = grass::Options::default().load_paths(&[dir, gen_dir]);
    grass::from_string(src, &opts).map_err(|e| anyhow::anyhow!("{}: {e}", entry.display()))
}

/// Called with `(key, message)` when an extra stylesheet fails to compile.
pub type ExtraErrorHook = Rc<dyn Fn(&str, &str)>;

/// Writes `<dir>/_<name>.scss` unless it already has this content.
pub fn write_partial(dir: &Path, name: &str, scss: &str) -> Result<bool> {
    let path = dir.join(format!("_{name}.scss"));
    if std::fs::read_to_string(&path).ok().as_deref() == Some(scss) {
        return Ok(false);
    }
    std::fs::write(&path, scss).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
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
    /// Extra stylesheets compiled before the user's (plugins), by key.
    extras: RefCell<BTreeMap<String, PathBuf>>,
    extra_error: RefCell<Option<ExtraErrorHook>>,
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
            // Above every theme source, including ~/.config/gtk-4.0/gtk.css
            // (USER): the default stylesheet resets all properties, so the
            // desktop's GTK theme never reaches hyprwidget's widgets.
            base: make(gtk4::STYLE_PROVIDER_PRIORITY_USER + 1, "default"),
            user: make(gtk4::STYLE_PROVIDER_PRIORITY_USER + 2, "user"),
            inline: make(gtk4::STYLE_PROVIDER_PRIORITY_USER + 3, "inline"),
            gen_dir,
            entry: RefCell::new(entry),
            inline_rules: RefCell::new(BTreeMap::new()),
            next_inline: Cell::new(0),
            extras: RefCell::default(),
            extra_error: RefCell::default(),
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

    /// Writes the generated partial `_<name>.scss` next to `_hypr.scss` (so
    /// the user's SCSS can `@import "<name>"`). Returns whether it changed.
    pub fn write_partial(&self, name: &str, scss: &str) -> Result<bool> {
        write_partial(&self.gen_dir, name, scss)
    }

    /// Adds (`Some`) or removes an extra stylesheet; takes effect on the
    /// next [`Self::reload_user`].
    pub fn set_extra(&self, key: &str, entry: Option<PathBuf>) {
        match entry {
            Some(p) => self.extras.borrow_mut().insert(key.to_owned(), p),
            None => self.extras.borrow_mut().remove(key),
        };
    }

    pub fn set_extra_error_hook(&self, f: ExtraErrorHook) {
        *self.extra_error.borrow_mut() = Some(f);
    }

    /// Path of the generated partials (`_hypr.scss`, `_hw-theme.scss`…).
    pub fn gen_dir(&self) -> &Path {
        &self.gen_dir
    }

    /// Recompiles the user's stylesheet (after the extra ones). On error the
    /// previous CSS stays; a broken extra stylesheet is only skipped.
    pub fn reload_user(&self) -> Result<()> {
        let entry = self.entry.borrow().clone();
        let style_dir = self.style_dir();
        let mut css = String::new();
        let extras = self.extras.borrow().clone();
        for (key, path) in extras {
            match compile_extra(&path, &self.gen_dir) {
                Ok(c) => {
                    css.push_str(&c);
                    css.push('\n');
                }
                Err(e) => {
                    tracing::error!("{key} stylesheet: {e:#}");
                    let hook = self.extra_error.borrow().clone();
                    if let Some(h) = hook {
                        h(&key, &format!("{e:#}"));
                    }
                }
            }
        }
        css.push_str(&compile_file(&entry, &[&style_dir, &self.gen_dir])?);
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
