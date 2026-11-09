//! `.socket2.sock` event lines (`name>>a,b,c`) and which parts of the
//! state each event invalidates.

/// One parsed event. `args` are split by commas up to the number of fields
/// the event has, so titles containing commas stay whole.
#[derive(Debug, PartialEq, Eq)]
pub struct Event<'a> {
    pub name: &'a str,
    pub data: &'a str,
    pub args: Vec<&'a str>,
}

/// Number of comma-separated fields per known event (the last field takes
/// the rest of the line).
fn arity(name: &str) -> usize {
    match name {
        "configreloaded" => 0,
        "workspacev2" | "focusedmon" | "focusedmonv2" | "activewindow" | "createworkspacev2"
        | "destroyworkspacev2" | "moveworkspace" | "renameworkspace" | "activespecial"
        | "activelayout" | "movewindow" | "changefloatingmode" | "screencast" | "windowtitlev2"
        | "togglegroup" | "pin" | "minimized" => 2,
        "monitoraddedv2" | "monitorremovedv2" | "moveworkspacev2" | "activespecialv2"
        | "movewindowv2" => 3,
        "openwindow" => 4,
        _ => 1,
    }
}

pub fn parse(line: &str) -> Event<'_> {
    let line = line.trim_end_matches(['\n', '\r']);
    let (name, data) = line.split_once(">>").unwrap_or((line, ""));
    let n = arity(name);
    let args = if n == 0 {
        Vec::new()
    } else {
        data.splitn(n, ',').collect()
    };
    Event { name, data, args }
}

/// Parts of the Hyprland state to refetch.
pub mod dirty {
    pub const WORKSPACES: u8 = 1;
    pub const MONITORS: u8 = 2;
    pub const CLIENTS: u8 = 4;
    pub const ACTIVE_WINDOW: u8 = 8;
    pub const ACTIVE_WORKSPACE: u8 = 16;
    pub const ALL: u8 = 31;
}

/// What `name` invalidates.
pub fn affects(name: &str) -> u8 {
    use dirty::{ACTIVE_WINDOW, ACTIVE_WORKSPACE, ALL, CLIENTS, MONITORS, WORKSPACES};
    match name {
        "workspace" | "workspacev2" | "focusedmon" | "focusedmonv2" => {
            MONITORS | ACTIVE_WORKSPACE | WORKSPACES
        }
        "createworkspace" | "createworkspacev2" | "destroyworkspace" | "destroyworkspacev2"
        | "moveworkspace" | "moveworkspacev2" | "renameworkspace" => {
            WORKSPACES | MONITORS | ACTIVE_WORKSPACE
        }
        "activespecial" | "activespecialv2" => MONITORS | WORKSPACES,
        "monitoradded" | "monitoraddedv2" | "monitorremoved" | "monitorremovedv2"
        | "configreloaded" => ALL,
        "activewindow" | "activewindowv2" => ACTIVE_WINDOW | ACTIVE_WORKSPACE,
        "openwindow" | "closewindow" | "movewindow" | "movewindowv2" | "kill" => {
            CLIENTS | WORKSPACES | ACTIVE_WINDOW | ACTIVE_WORKSPACE
        }
        "windowtitle" | "windowtitlev2" => CLIENTS | WORKSPACES | ACTIVE_WINDOW,
        "fullscreen" => CLIENTS | WORKSPACES | ACTIVE_WINDOW | ACTIVE_WORKSPACE,
        "changefloatingmode" | "pin" | "minimized" | "urgent" | "togglegroup" | "moveintogroup"
        | "moveoutofgroup" => CLIENTS | ACTIVE_WINDOW,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::dirty::*;
    use super::*;

    #[test]
    fn splits_known_fields() {
        let e = parse("workspacev2>>3,3\n");
        assert_eq!(e.name, "workspacev2");
        assert_eq!(e.args, ["3", "3"]);

        let e = parse("activespecialv2>>-98,special:special,DP-2");
        assert_eq!(e.args, ["-98", "special:special", "DP-2"]);
    }

    #[test]
    fn last_field_keeps_commas() {
        let e = parse("openwindow>>5615dff42cf0,5,kitty,vim a.rs, b.rs");
        assert_eq!(e.args, ["5615dff42cf0", "5", "kitty", "vim a.rs, b.rs"]);

        let e = parse("windowtitlev2>>5615dff42cf0,Messenger | Facebook, Zen");
        assert_eq!(e.args, ["5615dff42cf0", "Messenger | Facebook, Zen"]);

        let e = parse("activewindow>>zen,a, b, c");
        assert_eq!(e.args, ["zen", "a, b, c"]);
    }

    #[test]
    fn single_field_and_unknown_events_are_whole() {
        assert_eq!(parse("closewindow>>5615dff42cf0").args, ["5615dff42cf0"]);
        assert_eq!(parse("fullscreen>>1").args, ["1"]);
        let e = parse("someplugin>>x,y,z");
        assert_eq!(e.name, "someplugin");
        assert_eq!(e.data, "x,y,z");
        assert_eq!(e.args, ["x,y,z"]);
    }

    #[test]
    fn no_data() {
        let e = parse("configreloaded>>");
        assert_eq!(e.name, "configreloaded");
        assert_eq!(e.args, Vec::<&str>::new());
        let e = parse("bogus");
        assert_eq!((e.name, e.data), ("bogus", ""));
    }

    #[test]
    fn empty_trailing_fields() {
        // activewindow with no focused window: `activewindow>>,`
        assert_eq!(parse("activewindow>>,").args, ["", ""]);
        assert_eq!(parse("activewindowv2>>").args, [""]);
    }

    #[test]
    fn invalidation() {
        assert_eq!(affects("activewindowv2") & ACTIVE_WINDOW, ACTIVE_WINDOW);
        assert_eq!(affects("activewindowv2") & CLIENTS, 0);
        assert_ne!(affects("openwindow") & CLIENTS, 0);
        assert_ne!(affects("fullscreen") & CLIENTS, 0);
        assert_ne!(affects("activespecialv2") & MONITORS, 0);
        assert_ne!(affects("focusedmonv2") & ACTIVE_WORKSPACE, 0);
        assert_eq!(affects("configreloaded"), ALL);
        assert_eq!(affects("monitoraddedv2"), ALL);
        assert_eq!(affects("activelayout"), 0);
        assert_eq!(affects("submap"), 0);
    }
}
