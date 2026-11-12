//! song.ini values that change how notes are parsed. Copied from Truss (src/trackhash.rs).

use crate::chart::Modifiers;

/// JavaScript's `parseFloat`: the longest numeric prefix, ignoring leading whitespace.
fn parse_float(s: &str) -> Option<f64> {
    let s = s.trim_start();
    let mut end = 0;
    let mut best = None;
    for (i, _) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
        if i == 0 {
            continue;
        }
        if let Ok(v) = s[..i].parse::<f64>()
            && !s[..i].ends_with(['e', 'E', '+', '-'])
            && !s[..i].eq_ignore_ascii_case("inf")
            && !s[..i].eq_ignore_ascii_case("nan")
        {
            best = Some(v);
            end = i;
        }
    }
    let _ = end;
    best
}

/// The song.ini values that change parsing, read the way scan-chart reads them: keys are
/// case-sensitive, the last duplicate wins, and -1 or junk means "use the default".
pub fn modifiers_from_ini(pairs: &[(String, String)]) -> Modifiers {
    let get = |k: &str| pairs.iter().rev().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    let int = |keys: &[&str], d: i64| {
        for k in keys {
            if let Some(v) = get(k).and_then(parse_float)
                && v != -1.0
            {
                return v.round() as i64;
            }
        }
        d
    };
    let flag = |k: &str| match get(k).map(|v| v.to_ascii_lowercase()) {
        Some(v) if v == "true" || v == "1" => true,
        _ => false,
    };
    let d = Modifiers::default();
    let mut multiplier_note = int(&["multiplier_note", "star_power_note"], d.multiplier_note);
    if ![0, 103, 116].contains(&multiplier_note) {
        multiplier_note = 0;
    }
    Modifiers {
        hopo_frequency: int(&["hopo_frequency", "hopofreq"], d.hopo_frequency),
        eighthnote_hopo: flag("eighthnote_hopo"),
        multiplier_note,
        sustain_cutoff_threshold: int(&["sustain_cutoff_threshold"], d.sustain_cutoff_threshold),
        chord_snap_threshold: int(&["chord_snap_threshold"], d.chord_snap_threshold),
        five_lane_drums: flag("five_lane_drums"),
        pro_drums: flag("pro_drums"),
    }
}
