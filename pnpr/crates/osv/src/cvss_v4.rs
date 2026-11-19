#[rustfmt::skip]
const MACRO_VECTOR_TENTHS: [u8; 270] = [
    100, 98, 95, 100, 93, 91, 93, 89, 81, 99, 95, 92, 96, 87, 81, 90, 80, 68,
    98, 95, 90, 93, 89, 81, 88, 78, 69, 95, 92, 84, 92, 81, 65, 80, 70, 48,
    92, 82, 72, 79, 69, 50, 69, 55, 27, 99, 95, 92, 95, 90, 84, 92, 82, 72,
    97, 92, 85, 91, 83, 71, 81, 71, 53, 95, 92, 85, 92, 80, 70, 84, 71, 50,
    93, 85, 73, 82, 72, 59, 70, 52, 30, 86, 75, 52, 71, 52, 29, 63, 29, 17,
    98, 94, 91, 94, 86, 77, 87, 74, 63, 95, 87, 81, 89, 74, 64, 75, 63, 49,
    94, 88, 76, 86, 74, 59, 72, 57, 52, 89, 77, 67, 76, 58, 50, 57, 52, 25,
    83, 70, 54, 65, 58, 26, 53, 21, 13, 95, 88, 76, 90, 75, 61, 77, 68, 52,
    90, 76, 70, 77, 62, 53, 66, 59, 30, 89, 76, 62, 74, 57, 47, 61, 57, 24,
    78, 67, 58, 59, 57, 23, 52, 29, 16, 71, 59, 30, 58, 26, 15, 23, 13, 6,
    93, 86, 75, 86, 74, 56, 70, 52, 40, 87, 72, 58, 74, 61, 34, 54, 40, 22,
    85, 74, 62, 72, 55, 46, 53, 34, 19, 75, 55, 51, 57, 41, 19, 36, 19, 8,
    64, 51, 20, 47, 21, 11, 24, 9, 4, 88, 73, 60, 73, 59, 41, 54, 45, 20,
    75, 53, 50, 55, 40, 20, 43, 22, 11, 75, 58, 40, 61, 48, 20, 46, 17, 8,
    55, 45, 21, 51, 18, 9, 18, 7, 2, 53, 24, 14, 24, 12, 5, 10, 3, 1,
];

const METRIC_NAMES: [&str; 26] = [
    "AV", "AC", "AT", "PR", "UI", "VC", "VI", "VA", "SC", "SI", "SA", "E", "CR", "IR", "AR", "MAV",
    "MAC", "MAT", "MPR", "MUI", "MVC", "MVI", "MVA", "MSC", "MSI", "MSA",
];

const IDX_AV: usize = 0;
const IDX_AC: usize = 1;
const IDX_AT: usize = 2;
const IDX_PR: usize = 3;
const IDX_UI: usize = 4;
const IDX_VC: usize = 5;
const IDX_VI: usize = 6;
const IDX_VA: usize = 7;
const IDX_SC: usize = 8;
const IDX_SI: usize = 9;
const IDX_SA: usize = 10;
const IDX_E: usize = 11;
const IDX_CR: usize = 12;
const IDX_IR: usize = 13;
const IDX_AR: usize = 14;
const MODIFIED_OFFSET: usize = 15;

struct ParsedVector<'a>([Option<&'a str>; METRIC_NAMES.len()]);

impl<'a> ParsedVector<'a> {
    fn parse(body: &'a str) -> Option<Self> {
        if body.is_empty() {
            return None;
        }
        let mut slots = [None; METRIC_NAMES.len()];
        for part in body.split('/') {
            let (name, val) = part.split_once(':')?;
            if val.is_empty() {
                return None;
            }
            if let Some(metric_idx) = METRIC_NAMES
                .iter()
                .position(|&known| known == name)
                && slots[metric_idx].replace(val).is_some()
            {
                return None;
            }
        }
        if slots[..=IDX_SA].iter().any(Option::is_none) {
            return None;
        }
        Some(Self(slots))
    }

    fn raw(&self, idx: usize) -> Option<&'a str> {
        self.0[idx]
    }

    fn effective(&self, base_idx: usize) -> Option<&'a str> {
        self.raw(base_idx + MODIFIED_OFFSET)
            .filter(|&val| val != "X")
            .or_else(|| self.raw(base_idx))
    }

    fn optional_with_default(&self, idx: usize, default: &'static str) -> &'a str {
        self.raw(idx)
            .filter(|&val| val != "X")
            .unwrap_or(default)
    }
}

struct EffectiveRanks {
    exploit: [u8; 5],
    vuln: [u8; 3],
    sub: [u8; 3],
    reqs: [u8; 3],
    maturity: u8,
}

impl EffectiveRanks {
    fn from_parsed(parsed: &ParsedVector<'_>) -> Option<Self> {
        let exploit = parse_exploit_ranks(parsed)?;
        let vuln = [
            cia_rank(parsed.effective(IDX_VC)?)?,
            cia_rank(parsed.effective(IDX_VI)?)?,
            cia_rank(parsed.effective(IDX_VA)?)?,
        ];
        let sub = parse_subsequent_ranks(parsed)?;
        let reqs = [
            req_rank(parsed.optional_with_default(IDX_CR, "H"))?,
            req_rank(parsed.optional_with_default(IDX_IR, "H"))?,
            req_rank(parsed.optional_with_default(IDX_AR, "H"))?,
        ];
        let maturity = match parsed.optional_with_default(IDX_E, "A") {
            "A" => 0,
            "P" => 1,
            "U" => 2,
            _ => return None,
        };
        Some(Self { exploit, vuln, sub, reqs, maturity })
    }

    fn has_zero_impact(&self) -> bool {
        self.vuln == [2, 2, 2] && self.sub == [3, 3, 3]
    }

    fn macro_vector(&self) -> [u8; 6] {
        let [av, pr, ui, ac, at] = self.exploit;
        let eq1 = eq1_level(av, pr, ui);
        let eq2 = u8::from(!(ac == 0 && at == 0));
        let eq3 = eq3_level(self.vuln);
        let eq4 = eq4_level(self.sub);
        let has_high_req_impact = (0..3).any(|idx| self.reqs[idx] == 0 && self.vuln[idx] == 0);
        let eq6 = u8::from(!has_high_req_impact);
        [eq1, eq2, eq3, eq4, self.maturity, eq6]
    }
}

fn eq1_level(av: u8, pr: u8, ui: u8) -> u8 {
    if av == 0 && pr == 0 && ui == 0 {
        0
    } else if av != 3 && (av == 0 || pr == 0 || ui == 0) {
        1
    } else {
        2
    }
}

fn eq3_level(vuln: [u8; 3]) -> u8 {
    if vuln[0] == 0 && vuln[1] == 0 {
        0
    } else if vuln.contains(&0) {
        1
    } else {
        2
    }
}

fn eq4_level(sub: [u8; 3]) -> u8 {
    if sub[1] == 0 || sub[2] == 0 {
        0
    } else if sub.contains(&1) {
        1
    } else {
        2
    }
}

fn parse_exploit_ranks(parsed: &ParsedVector<'_>) -> Option<[u8; 5]> {
    let av = match parsed.effective(IDX_AV)? {
        "N" => 0,
        "A" => 1,
        "L" => 2,
        "P" => 3,
        _ => return None,
    };
    let pr = match parsed.effective(IDX_PR)? {
        "N" => 0,
        "L" => 1,
        "H" => 2,
        _ => return None,
    };
    let ui = match parsed.effective(IDX_UI)? {
        "N" => 0,
        "P" => 1,
        "A" => 2,
        _ => return None,
    };
    let ac = match parsed.effective(IDX_AC)? {
        "L" => 0,
        "H" => 1,
        _ => return None,
    };
    let at = match parsed.effective(IDX_AT)? {
        "N" => 0,
        "P" => 1,
        _ => return None,
    };
    Some([av, pr, ui, ac, at])
}

fn parse_subsequent_ranks(parsed: &ParsedVector<'_>) -> Option<[u8; 3]> {
    let _ = cia_rank(parsed.raw(IDX_SI)?)?;
    let _ = cia_rank(parsed.raw(IDX_SA)?)?;
    let sc = cia_rank(parsed.effective(IDX_SC)?)? + 1;
    let si = sub_safety_rank(parsed.effective(IDX_SI)?)?;
    let sa = sub_safety_rank(parsed.effective(IDX_SA)?)?;
    Some([sc, si, sa])
}

fn cia_rank(val: &str) -> Option<u8> {
    match val {
        "H" => Some(0),
        "L" => Some(1),
        "N" => Some(2),
        _ => None,
    }
}

fn sub_safety_rank(val: &str) -> Option<u8> {
    match val {
        "S" => Some(0),
        "H" => Some(1),
        "L" => Some(2),
        "N" => Some(3),
        _ => None,
    }
}

fn req_rank(val: &str) -> Option<u8> {
    match val {
        "H" => Some(0),
        "M" => Some(1),
        "L" => Some(2),
        _ => None,
    }
}

pub(super) fn cvss_v4_score(vector: &str) -> Option<f64> {
    let body = vector.strip_prefix("CVSS:4.0/")?;
    let parsed = ParsedVector::parse(body)?;
    let ranks = EffectiveRanks::from_parsed(&parsed)?;
    if ranks.has_zero_impact() {
        return Some(0.0);
    }
    let macro_vec = ranks.macro_vector();
    let base = lookup_macro_score(macro_vec)?;
    let mean_delta = mean_proportional_distance(&ranks, macro_vec, base);
    let clamped = (base - mean_delta).clamp(0.0, 10.0);
    Some((clamped * 10.0).round() / 10.0)
}

fn lookup_macro_score(eq: [u8; 6]) -> Option<f64> {
    if eq[0] >= 3 || eq[1] >= 2 || eq[3] >= 3 || eq[4] >= 3 {
        return None;
    }
    let eq3eq6_slot = match (eq[2], eq[5]) {
        (0, 0) => 0,
        (0, 1) => 1,
        (1, 0) => 2,
        (1, 1) => 3,
        (2, 1) => 4,
        _ => return None,
    };
    let idx = ((((eq[0] as usize) * 2 + eq[1] as usize) * 5 + eq3eq6_slot) * 3 + eq[3] as usize)
        * 3
        + eq[4] as usize;
    Some(f64::from(MACRO_VECTOR_TENTHS[idx]) / 10.0)
}

fn mean_proportional_distance(ranks: &EffectiveRanks, eq: [u8; 6], base: f64) -> f64 {
    let lower_scores = [
        lookup_macro_score([eq[0] + 1, eq[1], eq[2], eq[3], eq[4], eq[5]]),
        lookup_macro_score([eq[0], eq[1] + 1, eq[2], eq[3], eq[4], eq[5]]),
        next_lower_eq3eq6_score(eq),
        lookup_macro_score([eq[0], eq[1], eq[2], eq[3] + 1, eq[4], eq[5]]),
        lookup_macro_score([eq[0], eq[1], eq[2], eq[3], eq[4] + 1, eq[5]]),
    ];
    let proportions = [
        eq1_proportion(&ranks.exploit[..3], eq[0]),
        eq2_proportion(&ranks.exploit[3..], eq[1]),
        eq3eq6_proportion(ranks.vuln, ranks.reqs, eq[2], eq[5]),
        eq4_proportion(ranks.sub, eq[3]),
        0.0,
    ];
    let mut sum = 0.0;
    let mut count = 0_u32;
    for (lower, proportion) in lower_scores.into_iter().zip(proportions) {
        if let Some(lower_score) = lower {
            count += 1;
            sum = (base - lower_score).mul_add(proportion, sum);
        }
    }
    if count == 0 { 0.0 } else { sum / f64::from(count) }
}

fn next_lower_eq3eq6_score(eq: [u8; 6]) -> Option<f64> {
    match (eq[2], eq[5]) {
        (0, 0) => {
            let left = lookup_macro_score([eq[0], eq[1], 0, eq[3], eq[4], 1]);
            let right = lookup_macro_score([eq[0], eq[1], 1, eq[3], eq[4], 0]);
            left.into_iter()
                .chain(right)
                .reduce(f64::max)
        }
        (0, 1) | (1, 0) => lookup_macro_score([eq[0], eq[1], 1, eq[3], eq[4], 1]),
        (1, 1) => lookup_macro_score([eq[0], eq[1], 2, eq[3], eq[4], 1]),
        _ => None,
    }
}

fn dominating_distance(actual: &[u8], candidates: &[&[u8]]) -> f64 {
    for candidate in candidates {
        if actual
            .iter()
            .zip(*candidate)
            .all(|(cur, max)| cur >= max)
        {
            let dist: u8 = actual
                .iter()
                .zip(*candidate)
                .map(|(cur, max)| cur - max)
                .sum();
            return f64::from(dist);
        }
    }
    0.0
}

fn eq1_proportion(av_pr_ui: &[u8], eq1: u8) -> f64 {
    match eq1 {
        0 => 0.0,
        1 => dominating_distance(av_pr_ui, &[&[1, 0, 0], &[0, 1, 0], &[0, 0, 1]]) / 4.0,
        _ => dominating_distance(av_pr_ui, &[&[3, 0, 0], &[1, 1, 1]]) / 5.0,
    }
}

fn eq2_proportion(ac_at: &[u8], eq2: u8) -> f64 {
    match eq2 {
        0 => 0.0,
        _ => dominating_distance(ac_at, &[&[1, 0], &[0, 1]]) / 2.0,
    }
}

fn eq3eq6_proportion(vuln: [u8; 3], reqs: [u8; 3], eq3: u8, eq6: u8) -> f64 {
    let combined = [vuln[0], vuln[1], vuln[2], reqs[0], reqs[1], reqs[2]];
    match (eq3, eq6) {
        (0, 0) => dominating_distance(&combined, &[&[0, 0, 0, 0, 0, 0]]) / 7.0,
        (0, 1) => dominating_distance(&combined, &[&[0, 0, 1, 1, 1, 0], &[0, 0, 0, 1, 1, 1]]) / 6.0,
        (1, 0) => dominating_distance(&combined, &[&[1, 0, 0, 0, 0, 0], &[0, 1, 0, 0, 0, 0]]) / 8.0,
        (1, 1) => {
            let maxes: &[&[u8]] = &[
                &[1, 0, 1, 0, 1, 0],
                &[1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 1, 0, 1],
                &[0, 1, 1, 1, 0, 0],
                &[1, 1, 0, 0, 0, 1],
            ];
            dominating_distance(&combined, maxes) / 8.0
        }
        _ => dominating_distance(&combined, &[&[1, 1, 1, 0, 0, 0]]) / 10.0,
    }
}

fn eq4_proportion(sub: [u8; 3], eq4: u8) -> f64 {
    match eq4 {
        0 => dominating_distance(&sub, &[&[1, 0, 0]]) / 6.0,
        1 => dominating_distance(&sub, &[&[1, 1, 1]]) / 5.0,
        _ => dominating_distance(&sub, &[&[2, 2, 2]]) / 4.0,
    }
}
