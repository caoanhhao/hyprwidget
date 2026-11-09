//! `hw.service.audio`: PipeWire/WirePlumber state.
//!
//! Reading: a worker thread runs `pw-dump --monitor` and keeps the object
//! graph (nodes, devices, the `default` metadata); writing goes through
//! `wpctl` (`set-volume`, `set-mute`, `set-default`).
//!
//! Why not `pipewire-rs`: it would build here (libpipewire 1.6 + clang), but
//! volume on hardware sinks/sources lives in the *device route*, which
//! WirePlumber's mixer API manages; reimplementing that with raw PipeWire
//! pods is a lot of code for the same result. `pw-dump`/`wpctl` ship with
//! PipeWire/WirePlumber and the plan (§2) allows them for this stage.
//!
//! Volumes are WirePlumber's "cubic" scale, same as `wpctl get-volume`
//! (1.0 = 100 %).

use std::collections::HashMap;
use std::io::BufReader;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

use crate::service::{Pusher, Service, arg_f64};

const PROPS: &[&str] = &[
    "available",
    "speaker",
    "microphone",
    "sinks",
    "sources",
    "streams",
    "recorders",
];

pub fn start() -> Result<Rc<Service>> {
    let mut svc = Service::new("audio", PROPS);
    svc.set("available", json!(false));
    svc.method("set_volume", |args| {
        let target = target(args.first())?;
        let v = arg_f64(&args, 1, "volume")?.clamp(0.0, 1.5);
        wpctl(&["set-volume", &target, &format!("{v:.3}")])
    });
    svc.method("set_mute", |args| {
        let target = target(args.first())?;
        let m = match args.get(1) {
            Some(Value::Bool(true)) => "1",
            Some(Value::Bool(false)) => "0",
            None | Some(Value::Null) => "toggle",
            Some(Value::String(s)) if s == "toggle" => "toggle",
            _ => bail!("argument 2 must be a boolean or \"toggle\""),
        };
        wpctl(&["set-mute", &target, m])
    });
    svc.method("toggle_mute", |args| {
        let target = target(args.first())?;
        wpctl(&["set-mute", &target, "toggle"])
    });
    svc.method("set_default", |args| {
        let target = target(args.first())?;
        wpctl(&["set-default", &target])
    });
    let svc = Rc::new(svc);
    let pusher = svc.pusher();
    std::thread::Builder::new()
        .name("hw-audio".into())
        .spawn(move || worker(pusher))?;
    Ok(svc)
}

/// Node id (number, `{ id = … }`) or `"speaker"`/`"sink"`,
/// `"microphone"`/`"source"` (the defaults).
fn target(v: Option<&Value>) -> Result<String> {
    Ok(match v {
        None | Some(Value::Null) => "@DEFAULT_AUDIO_SINK@".into(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Object(o)) => match o.get("id") {
            Some(Value::Number(n)) => n.to_string(),
            _ => bail!("node table without id"),
        },
        Some(Value::String(s)) => match s.as_str() {
            "speaker" | "sink" => "@DEFAULT_AUDIO_SINK@".into(),
            "microphone" | "source" => "@DEFAULT_AUDIO_SOURCE@".into(),
            s if s.parse::<u32>().is_ok() => s.into(),
            s => bail!("unknown audio target '{s}'"),
        },
        Some(_) => bail!("argument 1 must be a node id or \"speaker\"/\"microphone\""),
    })
}

/// Runs `wpctl` without blocking the main loop on its exit.
fn wpctl(args: &[&str]) -> Result<Value> {
    let mut child = Command::new("wpctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("running wpctl")?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(Value::Null)
}

fn worker(pusher: Pusher) {
    loop {
        if pusher.is_closed() {
            return;
        }
        match run_dump(&pusher) {
            Ok(false) => return,
            Ok(true) => {}
            Err(e) => tracing::warn!("hw.service.audio: pw-dump: {e:#}"),
        }
        if !pusher.set("available", json!(false)) {
            return;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// Follows one `pw-dump --monitor`. `Ok(false)`: the service is gone.
fn run_dump(pusher: &Pusher) -> Result<bool> {
    let mut child = Command::new("pw-dump")
        .args(["--monitor", "--no-colors"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawning pw-dump")?;
    let out = child.stdout.take().context("pw-dump stdout")?;
    let mut graph = Graph::default();
    let mut alive = true;
    let stream =
        serde_json::Deserializer::from_reader(BufReader::new(out)).into_iter::<Vec<Value>>();
    for batch in stream {
        let Ok(batch) = batch else { break };
        graph.apply(batch);
        let snap = graph.snapshot();
        let ok =
            pusher.set("available", json!(true)) && snap.into_iter().all(|(p, v)| pusher.set(p, v));
        if !ok {
            alive = false;
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(alive)
}

/// PipeWire objects by id, as `pw-dump` prints them.
#[derive(Default)]
struct Graph {
    objects: HashMap<u64, Value>,
}

impl Graph {
    /// Applies one `pw-dump` batch: `info: null` removes, otherwise the
    /// object's fields (and `info` fields) replace the old ones.
    fn apply(&mut self, batch: Vec<Value>) {
        for obj in batch {
            let Some(id) = obj["id"].as_u64() else {
                continue;
            };
            if obj.get("info") == Some(&Value::Null) && obj.get("metadata").is_none() {
                self.objects.remove(&id);
                continue;
            }
            let entry = self.objects.entry(id).or_insert_with(|| json!({}));
            let (Value::Object(old), Value::Object(new)) = (entry, obj) else {
                continue;
            };
            for (k, v) in new {
                match (old.get_mut(&k), v) {
                    (Some(Value::Object(oi)), Value::Object(ni)) if k == "info" => {
                        oi.extend(ni);
                    }
                    (_, v) => {
                        old.insert(k, v);
                    }
                }
            }
        }
    }

    fn default_name(&self, key: &str) -> Option<&str> {
        self.objects
            .values()
            .filter(|o| o["props"]["metadata.name"] == "default")
            .flat_map(|o| o["metadata"].as_array().into_iter().flatten())
            .find(|m| m["key"] == key)
            .and_then(|m| m["value"]["name"].as_str())
    }

    /// Volume/mute source: the device route for hardware nodes, else the
    /// node's own `Props`.
    fn volume_props<'a>(&'a self, node: &'a Value) -> Option<&'a Value> {
        let props = &node["info"]["props"];
        let route = props["device.id"].as_u64().and_then(|dev| {
            let route_dev = props["card.profile.device"].as_u64()?;
            let dir = match props["media.class"].as_str()? {
                c if c.contains("Sink") => "Output",
                _ => "Input",
            };
            self.objects.get(&dev)?["info"]["params"]["Route"]
                .as_array()?
                .iter()
                .find(|r| r["device"].as_u64() == Some(route_dev) && r["direction"] == dir)
                .map(|r| &r["props"])
        });
        route
            .filter(|r| r["channelVolumes"].is_array())
            .or_else(|| {
                node["info"]["params"]["Props"]
                    .as_array()?
                    .iter()
                    .find(|p| p["channelVolumes"].is_array())
            })
    }

    fn node_json(&self, node: &Value, kind: &str, default: Option<&str>) -> Value {
        let props = &node["info"]["props"];
        let s = |k: &str| props[k].as_str().map(str::to_owned);
        let vp = self.volume_props(node);
        let vols: Vec<f64> = vp
            .and_then(|p| p["channelVolumes"].as_array())
            .map(|a| a.iter().filter_map(Value::as_f64).map(f64::cbrt).collect())
            .unwrap_or_default();
        let volume = if vols.is_empty() {
            0.0
        } else {
            vols.iter().sum::<f64>() / vols.len() as f64
        };
        let name = s("node.name");
        let mut m = Map::new();
        m.insert("id".into(), node["id"].clone());
        m.insert("kind".into(), json!(kind));
        m.insert("name".into(), json!(name));
        m.insert(
            "description".into(),
            json!(
                s("node.description")
                    .or_else(|| s("node.nick"))
                    .or_else(|| s("application.name"))
                    .or_else(|| name.clone())
            ),
        );
        m.insert("volume".into(), json!((volume * 1000.0).round() / 1000.0));
        m.insert(
            "volumes".into(),
            json!(
                vols.iter()
                    .map(|v| (v * 1000.0).round() / 1000.0)
                    .collect::<Vec<_>>()
            ),
        );
        m.insert(
            "mute".into(),
            json!(vp.and_then(|p| p["mute"].as_bool()).unwrap_or(false)),
        );
        m.insert(
            "is_default".into(),
            json!(default.is_some() && name.as_deref() == default),
        );
        m.insert("media_class".into(), json!(s("media.class")));
        m.insert("app_name".into(), json!(s("application.name")));
        m.insert("media_name".into(), json!(s("media.name")));
        m.insert(
            "icon".into(),
            json!(s("application.icon-name").or_else(|| s("device.icon-name"))),
        );
        m.insert("state".into(), node["info"]["state"].clone());
        Value::Object(m)
    }

    fn snapshot(&self) -> Vec<(&'static str, Value)> {
        let def_sink = self.default_name("default.audio.sink");
        let def_source = self.default_name("default.audio.source");
        let mut nodes: Vec<&Value> = self
            .objects
            .values()
            .filter(|o| o["type"] == "PipeWire:Interface:Node")
            .collect();
        nodes.sort_by_key(|o| o["id"].as_u64());
        let (mut sinks, mut sources, mut streams, mut recorders) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for n in nodes {
            match n["info"]["props"]["media.class"].as_str().unwrap_or("") {
                "Audio/Sink" => sinks.push(self.node_json(n, "sink", def_sink)),
                "Audio/Source" | "Audio/Source/Virtual" => {
                    sources.push(self.node_json(n, "source", def_source))
                }
                "Stream/Output/Audio" => streams.push(self.node_json(n, "playback", None)),
                "Stream/Input/Audio" => recorders.push(self.node_json(n, "record", None)),
                _ => {}
            }
        }
        let pick = |list: &[Value]| {
            list.iter()
                .find(|n| n["is_default"] == json!(true))
                .cloned()
                .unwrap_or(Value::Null)
        };
        vec![
            ("speaker", pick(&sinks)),
            ("microphone", pick(&sources)),
            ("sinks", Value::Array(sinks)),
            ("sources", Value::Array(sources)),
            ("streams", Value::Array(streams)),
            ("recorders", Value::Array(recorders)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<Value> {
        serde_json::from_str(
            r#"[
            { "id": 41, "type": "PipeWire:Interface:Metadata",
              "props": { "metadata.name": "default" },
              "metadata": [
                { "subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON",
                  "value": { "name": "hdmi" } },
                { "subject": 0, "key": "default.audio.source", "type": "Spa:String:JSON",
                  "value": { "name": "mic" } } ] },
            { "id": 49, "type": "PipeWire:Interface:Device",
              "info": { "params": { "Route": [
                { "index": 0, "direction": "Output", "device": 4,
                  "props": { "mute": false, "channelVolumes": [0.064, 0.064] } } ] } } },
            { "id": 44, "type": "PipeWire:Interface:Node",
              "info": { "state": "running", "props": {
                  "media.class": "Audio/Sink", "node.name": "hdmi",
                  "node.description": "HDMI", "device.id": 49, "card.profile.device": 4 },
                "params": { "Props": [ { "mute": false, "channelVolumes": [1.0, 1.0] } ] } } },
            { "id": 47, "type": "PipeWire:Interface:Node",
              "info": { "props": { "media.class": "Audio/Sink", "node.name": "spdif" },
                "params": { "Props": [ { "mute": true, "channelVolumes": [0.125] } ] } } },
            { "id": 55, "type": "PipeWire:Interface:Node",
              "info": { "props": { "media.class": "Audio/Source", "node.name": "mic" },
                "params": { "Props": [ { "mute": false, "channelVolumes": [1.0] } ] } } },
            { "id": 77, "type": "PipeWire:Interface:Node",
              "info": { "props": { "media.class": "Stream/Output/Audio", "node.name": "zen",
                  "application.name": "Zen", "media.name": "video" },
                "params": { "Props": [ { "mute": false, "channelVolumes": [0.008, 0.008] } ] } } }
        ]"#,
        )
        .unwrap()
    }

    fn snap(g: &Graph) -> HashMap<&'static str, Value> {
        g.snapshot().into_iter().collect()
    }

    #[test]
    fn defaults_volumes_and_kinds() {
        let mut g = Graph::default();
        g.apply(fixture());
        let s = snap(&g);
        // Route volume wins over the node's soft volume; cubic scale.
        assert_eq!(s["speaker"]["id"], json!(44));
        assert_eq!(s["speaker"]["volume"], json!(0.4));
        assert_eq!(s["speaker"]["description"], json!("HDMI"));
        assert_eq!(s["microphone"]["name"], json!("mic"));
        assert_eq!(s["sinks"].as_array().unwrap().len(), 2);
        assert_eq!(s["sinks"][1]["mute"], json!(true));
        assert_eq!(s["sinks"][1]["volume"], json!(0.5));
        assert_eq!(s["streams"][0]["volume"], json!(0.2));
        assert_eq!(s["streams"][0]["description"], json!("Zen"));
    }

    #[test]
    fn updates_and_removals() {
        let mut g = Graph::default();
        g.apply(fixture());
        // Partial update: only params change.
        g.apply(vec![json!({ "id": 49, "type": "PipeWire:Interface:Device",
            "info": { "params": { "Route": [ { "direction": "Output", "device": 4,
                "props": { "mute": true, "channelVolumes": [1.0, 1.0] } } ] } } })]);
        let s = snap(&g);
        assert_eq!(s["speaker"]["volume"], json!(1.0));
        assert_eq!(s["speaker"]["mute"], json!(true));
        g.apply(vec![json!({ "id": 77, "info": null })]);
        assert!(snap(&g)["streams"].as_array().unwrap().is_empty());
        // Default sink switch.
        g.apply(vec![
            json!({ "id": 41, "type": "PipeWire:Interface:Metadata",
            "props": { "metadata.name": "default" },
            "metadata": [ { "key": "default.audio.sink", "value": { "name": "spdif" } } ] }),
        ]);
        assert_eq!(snap(&g)["speaker"]["id"], json!(47));
    }

    #[test]
    fn targets() {
        assert_eq!(target(None).unwrap(), "@DEFAULT_AUDIO_SINK@");
        assert_eq!(
            target(Some(&json!("microphone"))).unwrap(),
            "@DEFAULT_AUDIO_SOURCE@"
        );
        assert_eq!(target(Some(&json!(44))).unwrap(), "44");
        assert_eq!(target(Some(&json!({ "id": 47 }))).unwrap(), "47");
        assert!(target(Some(&json!("nope"))).is_err());
    }
}
