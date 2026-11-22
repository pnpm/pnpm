use super::{
    Inline,
    Manifest,
    insert_top_level_block,
    locate,
    locate_mapping,
    mapping_keys,
    remove_mapping_entries,
    remove_top_level_block,
    render,
    upsert_sequence_entry,
};

const BLOCK: &str = "skills";
const DIRS: &str = "dirs";

/// Upsert `skills.dirs` toward `dirs`, creating the `skills` block if
/// absent. An empty `dirs` removes the `dirs` entry (and the `skills`
/// block when it held nothing else) because recording nothing means the
/// detected directories decide again. Returns whether anything changed.
pub(crate) fn set_skills_dirs(manifest: &mut Manifest, dirs: &[String]) -> bool {
    if dirs.is_empty() { remove_skills_dirs(manifest) } else { upsert_skills_dirs(manifest, dirs) }
}

/// The `dirs` entry removed, keeping any other entry the `skills` block
/// holds. Removing it when it is already absent (or when the block is)
/// changes nothing, so a write that learned nothing records nothing.
fn remove_skills_dirs(manifest: &mut Manifest) -> bool {
    let text = manifest.document.text();
    if locate(text, &[BLOCK]).is_none()
        || !mapping_keys(text, &[BLOCK])
            .iter()
            .any(|key| key == DIRS)
    {
        return false;
    }
    let new_text = remove_mapping_entries(text, &[BLOCK], &[DIRS.to_string()]);
    if mapping_keys(&new_text, &[BLOCK]).is_empty() {
        let new_text = remove_top_level_block(&new_text, BLOCK);
        manifest.document.keys.retain(|key| key != BLOCK);
        manifest.document.set_text(new_text);
        return true;
    }
    if new_text == text {
        return false;
    }
    manifest.document.set_text(new_text);
    true
}

/// The `dirs` entry upserted to `dirs`, into the `skills` block, created
/// when absent.
fn upsert_skills_dirs(manifest: &mut Manifest, dirs: &[String]) -> bool {
    let text = manifest.document.text();
    if recorded_skills_dirs(text) == Some(dirs.to_vec()) {
        return false;
    }
    let new_text = if locate(text, &[BLOCK]).is_none()
        && !matches!(locate_mapping(text, &[BLOCK]), Inline::Flow(_))
    {
        let mut block = format!("{BLOCK}:\n  {DIRS}:\n");
        for dir in dirs {
            block.push_str("    - ");
            block.push_str(&render::render_value(dir));
            block.push('\n');
        }
        let new_text = insert_top_level_block(manifest, BLOCK, &block);
        manifest.document.keys =
            render::target_order(&manifest.document.keys, &[BLOCK.to_string()]);
        new_text
    } else {
        upsert_sequence_entry(text, BLOCK, DIRS, dirs)
    };
    manifest.document.set_text(new_text);
    true
}

/// What the document records under `skills.dirs`, or `None` when absent.
fn recorded_skills_dirs(text: &str) -> Option<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct SkillsDirsProbe {
        #[serde(default)]
        skills: Option<SkillsDirsData>,
    }

    #[derive(serde::Deserialize)]
    struct SkillsDirsData {
        #[serde(default)]
        dirs: Option<Vec<String>>,
    }

    serde_saphyr::from_str::<SkillsDirsProbe>(text).ok()?.skills?.dirs
}

/// Upsert `globalPackagesConfig.skills.dirs` in the global `config.yaml`.
pub(crate) fn set_global_packages_skills_dirs(manifest: &mut Manifest, dirs: &[String]) -> bool {
    if dirs.is_empty() {
        return remove_global_packages_skills_dirs(manifest);
    }
    upsert_global_packages_skills_dirs(manifest, dirs)
}

fn remove_global_packages_skills_dirs(manifest: &mut Manifest) -> bool {
    let source = manifest.document.text();
    let path = ["globalPackagesConfig", "skills"];
    if locate(source, &path).is_none()
        || !mapping_keys(source, &path)
            .iter()
            .any(|key| key == DIRS)
    {
        return false;
    }
    let new_text = remove_mapping_entries(source, &path, &[DIRS.to_string()]);
    if mapping_keys(&new_text, &path).is_empty() {
        let new_text =
            remove_mapping_entries(&new_text, &["globalPackagesConfig"], &["skills".to_string()]);
        if mapping_keys(&new_text, &["globalPackagesConfig"]).is_empty() {
            let new_text = remove_top_level_block(&new_text, "globalPackagesConfig");
            manifest.document.keys.retain(|key| key != "globalPackagesConfig");
            manifest.document.set_text(new_text);
            return true;
        }
        manifest.document.set_text(new_text);
        return true;
    }
    if new_text == source {
        return false;
    }
    manifest.document.set_text(new_text);
    true
}

fn upsert_global_packages_skills_dirs(manifest: &mut Manifest, dirs: &[String]) -> bool {
    let source = manifest.document.text().to_owned();
    if recorded_global_packages_skills_dirs(&source).as_deref() == Some(dirs) {
        return false;
    }
    let new_text = if locate(&source, &["globalPackagesConfig"]).is_none() {
        let mut block = String::from("globalPackagesConfig:\n  skills:\n    dirs:\n");
        for dir in dirs {
            block.push_str("      - ");
            block.push_str(&render::render_value(dir));
            block.push('\n');
        }
        let new_text = insert_top_level_block(manifest, "globalPackagesConfig", &block);
        manifest.document.keys =
            render::target_order(&manifest.document.keys, &["globalPackagesConfig".to_string()]);
        new_text
    } else if locate(&source, &["globalPackagesConfig", "skills"]).is_none() {
        let mut block = String::from("  skills:\n    dirs:\n");
        for dir in dirs {
            block.push_str("      - ");
            block.push_str(&render::render_value(dir));
            block.push('\n');
        }
        insert_nested_block(&source, &["globalPackagesConfig"], &block)
    } else {
        upsert_nested_dirs(&source, dirs)
    };
    manifest.document.set_text(new_text);
    true
}

fn insert_nested_block(source: &str, parent: &[&str], block: &str) -> String {
    let Some(mapping) = locate(source, parent) else {
        return source.to_string();
    };
    let insert_at = mapping.entries
        .last()
        .map(|entry| entry.block_end)
        .unwrap_or(mapping.body_start);
    let mut out = source.to_string();
    out.insert_str(insert_at, block);
    out
}

fn upsert_nested_dirs(source: &str, dirs: &[String]) -> String {
    if let Some(skills) = locate(source, &["globalPackagesConfig", "skills"]) {
        if let Some(entry) = skills.entries
            .iter()
            .find(|entry| entry.key == DIRS)
        {
            let mut replacement = String::from("    dirs:\n");
            for dir in dirs {
                replacement.push_str("      - ");
                replacement.push_str(&render::render_value(dir));
                replacement.push('\n');
            }
            let mut out = source.to_string();
            out.replace_range(entry.line_start..entry.block_end, &replacement);
            return out;
        }
        let mut block = String::from("    dirs:\n");
        for dir in dirs {
            block.push_str("      - ");
            block.push_str(&render::render_value(dir));
            block.push('\n');
        }
        let insert_at = skills.entries
            .last()
            .map(|entry| entry.block_end)
            .unwrap_or(skills.body_start);
        let mut out = source.to_string();
        out.insert_str(insert_at, &block);
        return out;
    }
    source.to_string()
}

fn recorded_global_packages_skills_dirs(text: &str) -> Option<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Root {
        #[serde(default, rename = "globalPackagesConfig")]
        global_packages_config: Option<Global>,
    }
    #[derive(serde::Deserialize)]
    struct Global {
        #[serde(default)]
        skills: Option<Skills>,
    }
    #[derive(serde::Deserialize)]
    struct Skills {
        #[serde(default)]
        dirs: Option<Vec<String>>,
    }
    serde_saphyr::from_str::<Root>(text).ok()?.global_packages_config?.skills?.dirs
}
