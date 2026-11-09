//! power-profiles-daemon (`net.hadess.PowerProfiles`, system bus).

use std::cell::OnceCell;
use std::rc::Rc;

use hw_core::reactive::Signal;
use serde::Serialize;
use zbus::zvariant::Value;

use crate::dbus::{self, p_str, unwrap_variant, v_str};

const DEST: &str = "net.hadess.PowerProfiles";
const PATH: &str = "/net/hadess/PowerProfiles";
const IFACE: &str = "net.hadess.PowerProfiles";

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Profile {
    /// `power-saver`, `balanced`, `performance`.
    pub profile: String,
    pub driver: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct State {
    pub available: bool,
    pub active_profile: String,
    pub profiles: Vec<Profile>,
    /// Non-empty reason when performance is degraded (e.g. `lap-detected`).
    pub performance_degraded: String,
}

pub struct PowerProfiles {
    pub state: Signal<State>,
}

impl std::fmt::Debug for PowerProfiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PowerProfiles")
            .field("state", &self.state.peek())
            .finish()
    }
}

thread_local! {
    static INSTANCE: OnceCell<Rc<PowerProfiles>> = const { OnceCell::new() };
}

/// Started on first use (main thread only).
pub fn get() -> Rc<PowerProfiles> {
    INSTANCE.with(|i| {
        Rc::clone(i.get_or_init(|| {
            let this = Rc::new(PowerProfiles {
                state: Signal::new(State::default()),
            });
            let weak = Rc::downgrade(&this);
            dbus::spawn("power-profiles", async move {
                let conn = dbus::system().await?;
                dbus::watch_props(&conn, DEST, PATH, IFACE, |p| {
                    let Some(this) = weak.upgrade() else { return };
                    this.state.set(State {
                        available: true,
                        active_profile: p_str(p, "ActiveProfile"),
                        profiles: p
                            .get("Profiles")
                            .map(|v| parse_profiles(v))
                            .unwrap_or_default(),
                        performance_degraded: p_str(p, "PerformanceDegraded"),
                    });
                })
                .await
            });
            this
        }))
    })
}

/// `aa{sv}` → profiles.
fn parse_profiles(v: &Value<'_>) -> Vec<Profile> {
    let Value::Array(a) = unwrap_variant(v) else {
        return Vec::new();
    };
    a.iter()
        .filter_map(|d| {
            let Value::Dict(d) = unwrap_variant(d) else {
                return None;
            };
            let get = |k: &str| {
                d.get::<&str, Value<'_>>(&k)
                    .ok()
                    .flatten()
                    .and_then(|v| v_str(&v))
                    .unwrap_or_default()
            };
            Some(Profile {
                profile: get("Profile"),
                driver: get("Driver"),
            })
        })
        .collect()
}

impl PowerProfiles {
    /// Sets the active profile (may require polkit authorization).
    pub fn set_active(&self, profile: &str) {
        let profile = profile.to_owned();
        dbus::spawn("power-profiles set", async move {
            let conn = dbus::system().await?;
            let proxy = dbus::props_proxy(&conn, DEST, PATH).await?;
            proxy
                .set(
                    IFACE.try_into()?,
                    "ActiveProfile",
                    Value::from(profile.as_str()),
                )
                .await?;
            Ok(())
        });
    }
}
