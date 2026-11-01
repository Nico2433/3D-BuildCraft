//! The printers connected in the slicer and their live status, read from Moonraker (Klipper
//! printers such as a rooted Creality K1). Only status queries: nothing is sent that moves,
//! heats or prints.

use serde_json::Value;
use std::time::Duration;

/// A printer the slicer can send to: one it found on the network or a printer preset with a
/// print host. Its access code or API key is never read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printer {
    pub name: String,
    /// e.g. "Creality K1 Max"; empty when unknown.
    pub model: String,
    /// IP address or host name, maybe with a port.
    pub host: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// It does not answer.
    #[default]
    Offline,
    Idle,
    Printing,
    Paused,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub state: State,
    /// 0 to 1, while printing or paused.
    pub progress: Option<f32>,
    /// The file printing, or last printed; empty when none.
    pub file: String,
    /// Nozzle and bed temperatures in °C.
    pub nozzle: Option<f32>,
    pub bed: Option<f32>,
}

/// The printers connected in the slicer, without repeating a host.
pub fn connected(slicer: &super::Slicer) -> Vec<Printer> {
    super::profiles::Config::of(slicer).map_or_else(Vec::new, |c| c.connected())
}

/// Asks the printer's Moonraker for its status, waiting at most two seconds.
// shortcut: only Moonraker is read; OctoPrint, PrusaLink or a stock Creality's WebSocket on
// port 9999 show as offline. Add them when a user has one.
pub fn status(host: &str) -> Status {
    let host = host.trim().trim_end_matches('/');
    let mut base = if host.contains("://") {
        host.to_owned()
    } else {
        format!("http://{host}")
    };
    // Moonraker's own port unless the host names one (e.g. behind Mainsail on port 80).
    if !base.rsplit("//").next().unwrap_or("").contains(':') {
        base.push_str(":7125");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .into();
    let url = format!(
        "{base}/printer/objects/query?webhooks&print_stats&virtual_sdcard&extruder&heater_bed"
    );
    agent
        .get(&url)
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_string().ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .map_or_else(Status::default, |v: Value| parse(&v["result"]["status"]))
}

/// A Moonraker `objects/query` answer's `status`.
fn parse(s: &Value) -> Status {
    let job = s["print_stats"]["state"].as_str().unwrap_or("");
    let state = match (s["webhooks"]["state"].as_str(), job) {
        (Some("shutdown" | "error"), _) | (_, "error") => State::Error,
        (_, "printing") => State::Printing,
        (_, "paused") => State::Paused,
        _ => State::Idle,
    };
    let number = |v: &Value| v.as_f64().map(|n| n as f32);
    Status {
        state,
        progress: matches!(state, State::Printing | State::Paused)
            .then(|| number(&s["virtual_sdcard"]["progress"]))
            .flatten(),
        file: s["print_stats"]["filename"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
        nozzle: number(&s["extruder"]["temperature"]),
        bed: number(&s["heater_bed"]["temperature"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_moonraker_status() {
        let s: Value = serde_json::from_str(
            r#"{"webhooks": {"state": "ready"},
            "print_stats": {"filename": "Bone.gcode", "state": "printing"},
            "virtual_sdcard": {"progress": 0.25}, "extruder": {"temperature": 220.5},
            "heater_bed": {"temperature": 60.0}}"#,
        )
        .unwrap();
        let got = parse(&s);
        assert_eq!(got.state, State::Printing);
        assert_eq!(got.progress, Some(0.25));
        assert_eq!(got.file, "Bone.gcode");
        assert_eq!((got.nozzle, got.bed), (Some(220.5), Some(60.0)));
        let done = serde_json::json!({"print_stats": {"state": "complete"},
            "virtual_sdcard": {"progress": 1.0}});
        assert_eq!(parse(&done).state, State::Idle);
        assert_eq!(parse(&done).progress, None);
        let down = serde_json::json!({"webhooks": {"state": "shutdown"}});
        assert_eq!(parse(&down).state, State::Error);
        // Nothing listens on port 9 of localhost: it answers at once that it is offline.
        assert_eq!(status("127.0.0.1:9").state, State::Offline);
    }
}
