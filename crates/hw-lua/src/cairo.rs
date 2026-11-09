//! Cairo context for `DrawingArea { draw = function(cr, w, h) … end }`.

use gtk4::cairo;
use hw_ui::gtk4;
use mlua::{UserData, UserDataMethods};

pub struct Cairo(pub cairo::Context);

fn e(err: cairo::Error) -> mlua::Error {
    mlua::Error::runtime(format!("cairo: {err}"))
}

impl UserData for Cairo {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method(
            "set_source_rgba",
            |_, c, (r, g, b, a): (f64, f64, f64, Option<f64>)| {
                c.0.set_source_rgba(r, g, b, a.unwrap_or(1.0));
                Ok(())
            },
        );
        m.add_method("set_source_rgb", |_, c, (r, g, b): (f64, f64, f64)| {
            c.0.set_source_rgb(r, g, b);
            Ok(())
        });
        // Any CSS color: "#ff0000", "rgba(…)", "red".
        m.add_method("set_source_color", |_, c, css: String| {
            let rgba = gtk4::gdk::RGBA::parse(css.as_str())
                .map_err(|_| mlua::Error::runtime(format!("invalid color '{css}'")))?;
            c.0.set_source_rgba(
                rgba.red() as f64,
                rgba.green() as f64,
                rgba.blue() as f64,
                rgba.alpha() as f64,
            );
            Ok(())
        });
        m.add_method("set_line_width", |_, c, w: f64| {
            c.0.set_line_width(w);
            Ok(())
        });
        m.add_method("set_line_cap", |_, c, cap: String| {
            c.0.set_line_cap(match cap.as_str() {
                "round" => cairo::LineCap::Round,
                "square" => cairo::LineCap::Square,
                _ => cairo::LineCap::Butt,
            });
            Ok(())
        });
        m.add_method("move_to", |_, c, (x, y): (f64, f64)| {
            c.0.move_to(x, y);
            Ok(())
        });
        m.add_method("line_to", |_, c, (x, y): (f64, f64)| {
            c.0.line_to(x, y);
            Ok(())
        });
        m.add_method("rel_line_to", |_, c, (x, y): (f64, f64)| {
            c.0.rel_line_to(x, y);
            Ok(())
        });
        m.add_method(
            "curve_to",
            |_, c, (x1, y1, x2, y2, x3, y3): (f64, f64, f64, f64, f64, f64)| {
                c.0.curve_to(x1, y1, x2, y2, x3, y3);
                Ok(())
            },
        );
        m.add_method("rectangle", |_, c, (x, y, w, h): (f64, f64, f64, f64)| {
            c.0.rectangle(x, y, w, h);
            Ok(())
        });
        m.add_method(
            "rounded_rectangle",
            |_, c, (x, y, w, h, r): (f64, f64, f64, f64, f64)| {
                use std::f64::consts::{FRAC_PI_2, PI};
                let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
                c.0.new_sub_path();
                c.0.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
                c.0.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
                c.0.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
                c.0.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
                c.0.close_path();
                Ok(())
            },
        );
        m.add_method(
            "arc",
            |_, c, (x, y, r, a1, a2): (f64, f64, f64, f64, f64)| {
                c.0.arc(x, y, r, a1, a2);
                Ok(())
            },
        );
        m.add_method(
            "arc_negative",
            |_, c, (x, y, r, a1, a2): (f64, f64, f64, f64, f64)| {
                c.0.arc_negative(x, y, r, a1, a2);
                Ok(())
            },
        );
        m.add_method("new_path", |_, c, ()| {
            c.0.new_path();
            Ok(())
        });
        m.add_method("close_path", |_, c, ()| {
            c.0.close_path();
            Ok(())
        });
        m.add_method("fill", |_, c, ()| c.0.fill().map_err(e));
        m.add_method("fill_preserve", |_, c, ()| c.0.fill_preserve().map_err(e));
        m.add_method("stroke", |_, c, ()| c.0.stroke().map_err(e));
        m.add_method("stroke_preserve", |_, c, ()| {
            c.0.stroke_preserve().map_err(e)
        });
        m.add_method("paint", |_, c, alpha: Option<f64>| match alpha {
            Some(a) => c.0.paint_with_alpha(a).map_err(e),
            None => c.0.paint().map_err(e),
        });
        m.add_method("clip", |_, c, ()| {
            c.0.clip();
            Ok(())
        });
        m.add_method("save", |_, c, ()| c.0.save().map_err(e));
        m.add_method("restore", |_, c, ()| c.0.restore().map_err(e));
        m.add_method("translate", |_, c, (x, y): (f64, f64)| {
            c.0.translate(x, y);
            Ok(())
        });
        m.add_method("scale", |_, c, (x, y): (f64, f64)| {
            c.0.scale(x, y);
            Ok(())
        });
        m.add_method("rotate", |_, c, a: f64| {
            c.0.rotate(a);
            Ok(())
        });
        m.add_method("set_font_size", |_, c, s: f64| {
            c.0.set_font_size(s);
            Ok(())
        });
        m.add_method(
            "select_font_face",
            |_, c, (family, bold): (String, Option<bool>)| {
                let weight = if bold.unwrap_or(false) {
                    cairo::FontWeight::Bold
                } else {
                    cairo::FontWeight::Normal
                };
                c.0.select_font_face(&family, cairo::FontSlant::Normal, weight);
                Ok(())
            },
        );
        m.add_method("show_text", |_, c, t: String| c.0.show_text(&t).map_err(e));
        m.add_method("text_extents", |lua, c, t: String| {
            let x = c.0.text_extents(&t).map_err(e)?;
            let r = lua.create_table()?;
            r.set("width", x.width())?;
            r.set("height", x.height())?;
            r.set("x_bearing", x.x_bearing())?;
            r.set("y_bearing", x.y_bearing())?;
            r.set("x_advance", x.x_advance())?;
            Ok(r)
        });
    }
}
