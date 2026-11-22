//! Port of the deterministic policy in `hidden_subprocess_kwargs`.
//!
//! The caller supplies the platform and process-group booleans, so this crate
//! launches no process and has no OS, filesystem, network, Multica, or git
//! dependency. Process launch (`run_hidden`, job objects, the console window
//! guard) stays in the Python module.
//!
//! Request: `{"is_windows": bool, "new_process_group": bool}`. `is_windows` is
//! required; `new_process_group` defaults to `false`.
//!
//! Result: `{"creationflags": int|null, "startupinfo": {"dwFlags", "wShowWindow"}|null}`.
//! Both fields are `null` off Windows (Python returns an empty dict there).

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Value};

/// `CREATE_NO_WINDOW`.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `CREATE_NEW_PROCESS_GROUP`.
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
/// `STARTF_USESHOWWINDOW`.
pub const STARTF_USESHOWWINDOW: u32 = 0x0000_0001;
/// `SW_HIDE`.
pub const SW_HIDE: u32 = 0;

/// Startup info fields the policy sets, as Python's `STARTUPINFO` exposes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupInfo {
    /// `dwFlags`.
    pub dw_flags: u32,
    /// `wShowWindow`.
    pub w_show_window: u32,
}

/// The hidden-window decision for one launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Process creation flags, or `None` when no kwargs apply.
    pub creationflags: Option<u32>,
    /// Startup info, or `None` when no kwargs apply.
    pub startupinfo: Option<StartupInfo>,
}

/// Decide the hidden-window kwargs for a launch on the given platform.
#[must_use]
pub const fn decide(is_windows: bool, new_process_group: bool) -> Decision {
    if !is_windows {
        return Decision {
            creationflags: None,
            startupinfo: None,
        };
    }
    let mut flags = CREATE_NO_WINDOW;
    if new_process_group {
        flags |= CREATE_NEW_PROCESS_GROUP;
    }
    Decision {
        creationflags: Some(flags),
        startupinfo: Some(StartupInfo {
            dw_flags: STARTF_USESHOWWINDOW,
            w_show_window: SW_HIDE,
        }),
    }
}

impl Decision {
    fn as_json(&self) -> Value {
        json!({
            "creationflags": self.creationflags,
            "startupinfo": self.startupinfo.map(|info| json!({
                "dwFlags": info.dw_flags,
                "wShowWindow": info.w_show_window,
            })),
        })
    }
}

fn bool_field(request: &Value, key: &str) -> Result<Option<bool>, OpsError> {
    match request.get(key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(OpsError::invalid_request(format!(
            "{key} must be a boolean"
        ))),
    }
}

/// Handle one JSON request and return the JSON decision.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    if !request.is_object() {
        return Err(OpsError::invalid_request("request must be a JSON object"));
    }
    let is_windows = bool_field(&request, "is_windows")?
        .ok_or_else(|| OpsError::invalid_request("is_windows is required"))?;
    let new_process_group = bool_field(&request, "new_process_group")?.unwrap_or(false);
    Ok(decide(is_windows, new_process_group).as_json())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_returns_nothing() {
        let d = decide(false, true);
        assert_eq!(d.creationflags, None);
        assert_eq!(d.startupinfo, None);
    }

    #[test]
    fn windows_sets_no_window_and_hidden_startup() {
        let d = decide(true, false);
        assert_eq!(d.creationflags, Some(0x0800_0000));
        let info = d.startupinfo.expect("startupinfo");
        assert_eq!((info.dw_flags, info.w_show_window), (1, 0));
    }

    #[test]
    fn windows_new_group_adds_flag() {
        let d = decide(true, true);
        assert_eq!(d.creationflags, Some(0x0800_0200));
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(handle(json!([])).is_err());
        assert!(handle(json!({})).is_err());
        assert!(handle(json!({"is_windows": "yes"})).is_err());
        assert!(handle(json!({"is_windows": true, "new_process_group": 1})).is_err());
    }

    #[test]
    fn group_defaults_to_false() {
        let out = handle(json!({"is_windows": true})).unwrap();
        assert_eq!(out["creationflags"], json!(0x0800_0000));
    }
}
