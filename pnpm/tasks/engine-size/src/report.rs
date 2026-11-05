use crate::{
    attribution::{Attribution, Entry},
    cli_args::Lto,
    install_timing::InstallTimings,
    sections::Section,
};
use serde::Serialize;
use std::fmt::Write as _;

#[derive(Debug, Serialize)]
pub struct Report {
    pub target: String,
    pub measurements: Vec<Measurement>,
}

#[derive(Debug, Serialize)]
pub struct Measurement {
    pub lto: Lto,
    pub stripped_size: u64,
    pub sections: Vec<Section>,
    pub attribution: Attribution,
    pub install: Option<InstallTimings>,
}

impl Report {
    pub fn to_markdown(&self, top: usize) -> String {
        let mut out = format!("# Engine size\n\nTarget: `{}`\n\n", self.target);
        out.push_str("| LTO | Stripped size | Code (symbols) | `pnpm_*` code | Cold install | Warm install |\n");
        out.push_str("| --- | ---: | ---: | ---: | ---: | ---: |\n");
        for measurement in &self.measurements {
            out.push_str(&measurement.summary_row());
        }
        for measurement in &self.measurements {
            measurement.write_details(&mut out, top);
        }
        out
    }
}

impl Measurement {
    fn summary_row(&self) -> String {
        let (cold, warm) = self.install
            .as_ref()
            .map_or_else(
                || ("-".to_string(), "-".to_string()),
                |timings| {
                    (
                        format!("{} ms", timings.cold_store_ms),
                        format!("{} ms", timings.warm_store_ms),
                    )
                },
            );
        format!(
            "| {} | {} | {} | {} | {cold} | {warm} |\n",
            self.lto.as_str(),
            bytes(self.stripped_size),
            bytes(self.attribution.code_size),
            bytes(self.attribution.pnpm_code_size),
        )
    }

    fn write_details(&self, out: &mut String, top: usize) {
        let lto = self.lto.as_str();
        let _ = write!(out, "\n## LTO {lto}: sections\n\n| Section | Size |\n| --- | ---: |\n");
        for section in self.sections.iter().take(top) {
            let _ = writeln!(out, "| `{}` | {} |", section.name, bytes(section.size));
        }
        write_entries(out, &format!("LTO {lto}: crates"), "Crate", &self.attribution.crates);
        write_entries(
            out,
            &format!("LTO {lto}: functions"),
            "Function",
            &self.attribution.functions,
        );
    }
}

fn write_entries(out: &mut String, heading: &str, column: &str, entries: &[Entry]) {
    let _ = write!(out, "\n## {heading}\n\n| {column} | Size | Symbols |\n| --- | ---: | ---: |\n");
    for entry in entries {
        let name = entry.name.replace('|', r"\|");
        let _ = writeln!(out, "| `{name}` | {} | {} |", bytes(entry.size), entry.symbols);
    }
}

/// `size` in KiB below 1 MiB and in MiB above, with the exact byte count.
fn bytes(size: u64) -> String {
    const KIB: f64 = 1024.0;
    let size_f = size as f64;
    if size_f < KIB * KIB {
        format!("{:.1} KiB ({size} B)", size_f / KIB)
    } else {
        format!("{:.2} MiB ({size} B)", size_f / KIB / KIB)
    }
}
