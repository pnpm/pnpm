use super::{
    FsReadHead, LinkBinsError, ShimSpec, ShimTargetCache, is_node_bin_name, is_shim_pointing_at,
    shim_writer::existing_shim_is_current,
};

pub(super) fn is_current<Sys>(
    spec: &ShimSpec<'_>,
    cache: &ShimTargetCache,
    existing_shim: Option<&str>,
) -> Result<bool, LinkBinsError>
where
    Sys: FsReadHead,
{
    if !spec.options.prefer_symlinked_executables || is_node_bin_name(spec.shim_path) {
        return Ok(false);
    }
    match existing_shim {
        Some(existing) if spec.node_path.is_empty() && !existing.contains("export NODE_PATH=") => {
            Ok(is_shim_pointing_at(existing, spec.shim_path, spec.target_path))
        }
        Some(existing) => {
            let runtime = spec.resolve_runtime::<Sys>(cache)?;
            let sh_body = spec.sh_body(runtime.as_ref())?;
            Ok(existing_shim_is_current(Some(existing), &sh_body, spec))
        }
        None => Ok(false),
    }
}
