//! The session's replies to the viewport's questions, read: what is under
//! the cursor (`pick`) and the snap points near it (`snap`). The commands
//! are in `cad-ffi`'s `session.rs`; this reads what they answer.

use crate::json::Json;

/// One snap point the core found near the cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapHit {
    /// `cad_core::snap::kind`'s code.
    pub kind: u32,
    /// Its name: "endpoint", "midpoint", ...
    pub name: String,
    pub point: [f64; 2],
    /// The entity the point is on.
    pub handle: u64,
}

/// The command line that asks for the snaps within `aperture` world units of
/// `at`, for the kinds in the bit mask `kinds`.
pub fn snap_command(at: [f64; 2], aperture: f64, kinds: u32) -> String {
    format!(
        "snap {} {} {} {}",
        num(at[0]),
        num(at[1]),
        num(aperture),
        kinds
    )
}

/// The command line that asks which entity is within `aperture` of `at`.
pub fn pick_command(at: [f64; 2], aperture: f64) -> String {
    format!("pick {} {} {}", num(at[0]), num(at[1]), num(aperture))
}

fn num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "0".to_string()
    }
}

pub fn parse_snaps(reply: &str) -> Result<Vec<SnapHit>, String> {
    let j = Json::parse(reply)?;
    let list = j
        .get("snaps")
        .and_then(Json::as_array)
        .ok_or("snap: no list in the reply")?;
    let mut out = Vec::new();
    for s in list {
        let x = s.get("x").and_then(Json::as_f64).ok_or("snap: no x")?;
        let y = s.get("y").and_then(Json::as_f64).ok_or("snap: no y")?;
        out.push(SnapHit {
            kind: s
                .get("kind")
                .and_then(Json::as_f64)
                .filter(|k| *k >= 0.0 && *k <= f64::from(u32::MAX))
                .map_or(0, |k| k as u32),
            name: s
                .get("name")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
            point: [x, y],
            handle: s
                .get("handle")
                .and_then(Json::as_str)
                .and_then(parse_handle)
                .unwrap_or(0),
        });
    }
    Ok(out)
}

/// The entity under the cursor, if any.
pub fn parse_pick(reply: &str) -> Result<Option<u64>, String> {
    let j = Json::parse(reply)?;
    match j.get("handle") {
        Some(Json::Null) => Ok(None),
        Some(Json::Str(s)) => parse_handle(s)
            .map(Some)
            .ok_or_else(|| "pick: not a handle".to_string()),
        _ => Err("pick: no handle in the reply".into()),
    }
}

/// A handle as the core writes it: upper-case hex, no prefix.
pub fn parse_handle(s: &str) -> Option<u64> {
    u64::from_str_radix(s, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_snaps() {
        let r = r#"{"snaps":[{"kind":2,"name":"endpoint","x":1.5,"y":-2,"handle":"2F"},{"kind":8,"name":"midpoint","x":0,"y":0,"handle":"A"}]}"#;
        let s = parse_snaps(r).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].kind, s[0].name.as_str(), s[0].point, s[0].handle),
            (2, "endpoint", [1.5, -2.0], 0x2F)
        );
        assert_eq!(s[1].handle, 0xA);
        assert_eq!(parse_snaps(r#"{"snaps":[]}"#).unwrap(), vec![]);
        for bad in ["", "{}", r#"{"snaps":[{"y":1}]}"#, r#"{"snaps":5}"#, "[1]"] {
            assert!(parse_snaps(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reads_picks() {
        assert_eq!(parse_pick(r#"{"handle":"2F"}"#), Ok(Some(0x2F)));
        assert_eq!(parse_pick(r#"{"handle":null}"#), Ok(None));
        for bad in ["", "{}", r#"{"handle":"zz"}"#, r#"{"handle":3}"#] {
            assert!(parse_pick(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn commands_are_what_the_session_reads() {
        assert_eq!(snap_command([1.5, -2.0], 0.25, 255), "snap 1.5 -2 0.25 255");
        assert_eq!(pick_command([0.0, 10.0], 3.0), "pick 0 10 3");
        assert_eq!(pick_command([f64::NAN, 1.0], f64::INFINITY), "pick 0 1 0");
    }

    #[test]
    fn the_session_answers_in_this_form() {
        let mut s = cad_ffi::session::Session::new();
        let id = s.sample().unwrap();
        let d = crate::display::Display::parse(&s.display(id).unwrap()).unwrap();
        let ext = d.extents().unwrap();
        // ask at the drawing's corner with a generous aperture and every kind
        let reply = s
            .exec(id, &snap_command(ext.min, ext.width().max(1.0), u32::MAX))
            .unwrap();
        let snaps = parse_snaps(&reply).unwrap();
        assert!(!snaps.is_empty(), "{reply}");
        assert!(
            snaps
                .iter()
                .all(|h| h.point[0].is_finite() && !h.name.is_empty())
        );
        let reply = s.exec(id, &pick_command(snaps[0].point, 1.0)).unwrap();
        assert!(parse_pick(&reply).is_ok(), "{reply}");
    }
}
