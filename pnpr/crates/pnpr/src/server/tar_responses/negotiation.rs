use super::{HeaderMap, header};

/// The coding a tar response is sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum TarCoding {
    Zstd,
    Gzip,
    Identity,
}

/// The coding the request's `Accept-Encoding` prefers among those pnpr sends
/// tars in. A higher quality wins, zstd breaks a tie, `*` stands for any coding
/// the header does not name, and `q=0` refuses one.
pub(in super::super) fn preferred_coding(headers: &HeaderMap) -> TarCoding {
    let mut qualities = Qualities::default();
    for entry in headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .flat_map(|value| {
            value
                .to_str()
                .unwrap_or_default()
                .split(',')
        })
    {
        qualities.add(entry);
    }
    let zstd = qualities.zstd.or(qualities.any).unwrap_or(0.0);
    let gzip = qualities.gzip.or(qualities.any).unwrap_or(0.0);
    if zstd > 0.0 && zstd >= gzip {
        TarCoding::Zstd
    } else if gzip > 0.0 {
        TarCoding::Gzip
    } else {
        TarCoding::Identity
    }
}

#[derive(Default)]
struct Qualities {
    zstd: Option<f32>,
    gzip: Option<f32>,
    any: Option<f32>,
}

impl Qualities {
    fn add(&mut self, entry: &str) {
        let mut parts = entry.split(';');
        let coding = parts
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let quality = parts
            .find_map(|param| param.trim().strip_prefix("q="))
            .map_or(Some(1.0), |quality| quality.trim().parse::<f32>().ok())
            .unwrap_or(0.0);
        let slot = match coding.as_str() {
            "zstd" => &mut self.zstd,
            "gzip" | "x-gzip" => &mut self.gzip,
            "*" => &mut self.any,
            _ => return,
        };
        *slot = Some(slot.map_or(quality, |known| known.max(quality)));
    }
}
