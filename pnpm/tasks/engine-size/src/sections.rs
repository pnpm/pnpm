use object::{Object, ObjectSection};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Section {
    pub name: String,
    pub size: u64,
}

/// The executable's non-empty sections, largest first.
pub fn read_sections(binary: &[u8]) -> Result<Vec<Section>, String> {
    let file = object::File::parse(binary).map_err(|error| format!("parse the probe: {error}"))?;
    let mut sections: Vec<Section> = file
        .sections()
        .filter(|section| section.size() > 0)
        .map(|section| Section {
            name: section
                .name()
                .unwrap_or("?")
                .to_string(),
            size: section.size(),
        })
        .collect();
    sections.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(sections)
}
