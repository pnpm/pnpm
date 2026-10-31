use super::{
    Inline, Manifest, insert_top_level_block, locate, locate_mapping, mapping_keys,
    remove_mapping_entries, remove_top_level_block, render, upsert_sequence_entry,
};

const BLOCK: &str = "skills";
const DIRS: &str = "dirs";

/// Upsert `skills.dirs` toward `dirs`, creating the `skills` block if
/// absent. An empty `dirs` removes the `dirs` entry — and the `skills`
/// block when it held nothing else — because recording nothing means the
/// detected directories decide again. Returns whether anything changed.
pub(crate) fn set_skills_dirs(manifest: &mut Manifest, dirs: &[String]) -> bool {
    if dirs.is_empty() { remove_skills_dirs(manifest) } else { upsert_skills_dirs(manifest, dirs) }
}

/// The `dirs` entry removed, keeping any other entry the `skills` block
/// holds. Removing it when it is already absent — or when the block is —
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

/// The `dirs` entry upserted to `dirs`, into the `skills` block — created
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
