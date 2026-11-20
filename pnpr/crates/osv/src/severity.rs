use serde::Deserialize;

use super::cvss_v4::cvss_v4_score;

#[derive(Deserialize)]
pub(super) struct OsvSeverity {
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) score: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SeverityLevel {
    Info,
    Low,
    Moderate,
    High,
    Critical,
}

impl SeverityLevel {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Low => "low",
            Self::Moderate => "moderate",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub(super) fn from_label(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "info" | "informational" | "none" => Some(Self::Info),
            "low" => Some(Self::Low),
            "moderate" | "medium" => Some(Self::Moderate),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }

    pub(super) fn from_score(score: f64) -> Option<Self> {
        if !score.is_finite() || !(0.0..=10.0).contains(&score) {
            return None;
        }
        if score >= 9.0 {
            Some(Self::Critical)
        } else if score >= 7.0 {
            Some(Self::High)
        } else if score >= 4.0 {
            Some(Self::Moderate)
        } else if score > 0.0 {
            Some(Self::Low)
        } else {
            Some(Self::Info)
        }
    }

    pub(super) fn from_osv_severity(entry: &OsvSeverity) -> Option<Self> {
        let kind = entry.kind.trim();
        let raw_score = entry.score.trim();
        let calculated = match kind {
            "CVSS_V4" => cvss_v4_score(raw_score),
            "CVSS_V3" => cvss_v3_score(raw_score),
            "CVSS_V2" => cvss_v2_score(raw_score),
            _ => cvss_any_score(raw_score),
        };
        calculated
            .or_else(|| raw_score.parse::<f64>().ok())
            .and_then(Self::from_score)
            .or_else(|| Self::from_label(raw_score))
    }
}

fn cvss_any_score(raw: &str) -> Option<f64> {
    cvss_v4_score(raw).or_else(|| cvss_v3_score(raw)).or_else(|| cvss_v2_score(raw))
}

#[derive(Debug, Clone, Copy)]
enum CvssV3Scope {
    Unchanged,
    Changed,
}

#[derive(Default)]
struct RawV3Slots<'a>([Option<&'a str>; 8]);

impl<'a> RawV3Slots<'a> {
    fn parse(body: &'a str) -> Option<Self> {
        if body.is_empty() {
            return None;
        }
        let mut slots = Self::default();
        for part in body.split('/') {
            let (name, val) = part.split_once(':')?;
            if val.is_empty() {
                return None;
            }
            if let Some(slot_idx) = v3_slot_index(name)
                && slots.0[slot_idx].replace(val).is_some()
            {
                return None;
            }
        }
        Some(slots)
    }
}

fn v3_slot_index(name: &str) -> Option<usize> {
    match name {
        "AV" => Some(0),
        "AC" => Some(1),
        "PR" => Some(2),
        "UI" => Some(3),
        "S" => Some(4),
        "C" => Some(5),
        "I" => Some(6),
        "A" => Some(7),
        _ => None,
    }
}

pub(super) fn cvss_v3_score(vector: &str) -> Option<f64> {
    let body = vector
        .strip_prefix("CVSS:3.1/")
        .or_else(|| vector.strip_prefix("CVSS:3.0/"))?;
    let slots = RawV3Slots::parse(body)?.0;
    let scope = match slots[4]? {
        "U" => CvssV3Scope::Unchanged,
        "C" => CvssV3Scope::Changed,
        _ => return None,
    };
    let exploitability = 8.22
        * v3_av_weight(slots[0]?)?
        * v3_ac_weight(slots[1]?)?
        * v3_pr_weight(slots[2]?, scope)?
        * v3_ui_weight(slots[3]?)?;
    let unaffected = (1.0 - v3_cia_weight(slots[5]?)?)
        * (1.0 - v3_cia_weight(slots[6]?)?)
        * (1.0 - v3_cia_weight(slots[7]?)?);
    let iss = 1.0 - unaffected;
    if iss <= 0.0 {
        return Some(0.0);
    }
    let raw_total = match scope {
        CvssV3Scope::Unchanged => 6.42_f64.mul_add(iss, exploitability).min(10.0),
        CvssV3Scope::Changed => {
            let scaled_iss = 7.52 * (iss - 0.029);
            let impact = 3.25_f64.mul_add(-(iss - 0.02).powi(15), scaled_iss);
            if impact <= 0.0 {
                return Some(0.0);
            }
            (1.08 * (impact + exploitability)).min(10.0)
        }
    };
    Some(roundup_tenths(raw_total))
}

fn roundup_tenths(value: f64) -> f64 {
    let scaled = (value * 100_000.0).round() as i64;
    if scaled % 10_000 == 0 {
        (scaled as f64) / 100_000.0
    } else {
        (((scaled / 10_000) + 1) as f64) / 10.0
    }
}

fn v3_av_weight(val: &str) -> Option<f64> {
    match val {
        "N" => Some(0.85),
        "A" => Some(0.62),
        "L" => Some(0.55),
        "P" => Some(0.20),
        _ => None,
    }
}

fn v3_ac_weight(val: &str) -> Option<f64> {
    match val {
        "L" => Some(0.77),
        "H" => Some(0.44),
        _ => None,
    }
}

fn v3_pr_weight(val: &str, scope: CvssV3Scope) -> Option<f64> {
    match (val, scope) {
        ("N", CvssV3Scope::Unchanged | CvssV3Scope::Changed) => Some(0.85),
        ("L", CvssV3Scope::Unchanged) => Some(0.62),
        ("L", CvssV3Scope::Changed) => Some(0.68),
        ("H", CvssV3Scope::Unchanged) => Some(0.27),
        ("H", CvssV3Scope::Changed) => Some(0.50),
        _ => None,
    }
}

fn v3_ui_weight(val: &str) -> Option<f64> {
    match val {
        "N" => Some(0.85),
        "R" => Some(0.62),
        _ => None,
    }
}

fn v3_cia_weight(val: &str) -> Option<f64> {
    match val {
        "H" => Some(0.56),
        "L" => Some(0.22),
        "N" => Some(0.0),
        _ => None,
    }
}

pub(super) fn cvss_v2_score(vector: &str) -> Option<f64> {
    let trimmed = vector
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(vector);
    let body = trimmed.strip_prefix("CVSS:2.0/").unwrap_or(trimmed);
    let slots = parse_v2_slots(body)?;
    let unaffected = (1.0 - slots[3]) * (1.0 - slots[4]) * (1.0 - slots[5]);
    let impact = 10.41 * (1.0 - unaffected);
    if impact <= 0.0 {
        return Some(0.0);
    }
    let exploitability = 20.0 * slots[0] * slots[1] * slots[2];
    let weighted = 0.6_f64.mul_add(impact, 0.4 * exploitability) - 1.5;
    let raw = weighted * 1.176;
    Some(((raw.clamp(0.0, 10.0) * 10.0).round()) / 10.0)
}

fn parse_v2_slots(body: &str) -> Option<[f64; 6]> {
    if !body.contains("AV:") {
        return None;
    }
    let mut slots = [None; 6];
    for part in body.split('/') {
        let (name, val) = part.split_once(':')?;
        if let Some(slot_idx) = v2_slot_index(name) {
            let weight = v2_metric_weight(name, val)?;
            if slots[slot_idx].replace(weight).is_some() {
                return None;
            }
        }
    }
    Some([slots[0]?, slots[1]?, slots[2]?, slots[3]?, slots[4]?, slots[5]?])
}

fn v2_slot_index(name: &str) -> Option<usize> {
    match name {
        "AV" => Some(0),
        "AC" => Some(1),
        "Au" => Some(2),
        "C" => Some(3),
        "I" => Some(4),
        "A" => Some(5),
        _ => None,
    }
}

fn v2_metric_weight(name: &str, val: &str) -> Option<f64> {
    match (name, val) {
        ("AV", "L") => Some(0.395),
        ("AV", "A") => Some(0.646),
        ("AV", "N") => Some(1.0),
        ("AC", "H") => Some(0.35),
        ("AC", "M") => Some(0.61),
        ("AC", "L") => Some(0.71),
        ("Au", "M") => Some(0.45),
        ("Au", "S") => Some(0.56),
        ("Au", "N") => Some(0.704),
        ("C" | "I" | "A", "N") => Some(0.0),
        ("C" | "I" | "A", "P") => Some(0.275),
        ("C" | "I" | "A", "C") => Some(0.660),
        _ => None,
    }
}
