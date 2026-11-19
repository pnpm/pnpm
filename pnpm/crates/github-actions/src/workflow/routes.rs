use super::{
    Component, Document, QueryError, Route,
    root::{ContainerSummary, DirectUses, StepField, StepSummary, parse_workflow_root},
};
use std::collections::HashMap;

const MAX_MERGE_DEPTH: usize = 8;

pub(super) fn uses_routes(
    text: &str,
    document: &Document,
) -> Result<Vec<Route<'static>>, QueryError> {
    let root = parse_workflow_root(text)?;
    let mut routes = Vec::new();
    for (job_name, job) in root.jobs {
        let prefix = ["jobs".into(), job_name.into()];
        add_container_routes(document, &mut routes, &prefix, &job, true)?;
    }
    match root.runs {
        Some(runs) => add_container_routes(document, &mut routes, &["runs".into()], &runs, false)?,
        None if root.has_merge => add_merged_runs_routes(document, &mut routes)?,
        None => {}
    }
    Ok(routes)
}

fn add_container_routes(
    document: &Document,
    routes: &mut Vec<Route<'static>>,
    prefix: &[Component<'static>],
    container: &ContainerSummary,
    include_container_uses: bool,
) -> Result<(), QueryError> {
    if include_container_uses {
        add_uses_for_summary(document, routes, prefix, container.uses, container.has_merge)?;
    }
    match &container.steps {
        Some(StepField::Sequence(steps)) => {
            let mut steps_prefix = prefix.to_vec();
            steps_prefix.push("steps".into());
            add_direct_step_routes(document, routes, &steps_prefix, steps)?;
        }
        Some(StepField::NonSequence) => {}
        None if container.has_merge => {
            if let Some(steps_prefix) = find_key_path(document, prefix, "steps")? {
                add_merged_step_routes(document, routes, &steps_prefix)?;
            }
        }
        None => {}
    }
    Ok(())
}

fn add_merged_runs_routes(
    document: &Document,
    routes: &mut Vec<Route<'static>>,
) -> Result<(), QueryError> {
    if let Some(runs_prefix) = find_key_path(document, &[], "runs")?
        && let Some(steps_prefix) = find_key_path(document, &runs_prefix, "steps")?
    {
        add_merged_step_routes(document, routes, &steps_prefix)?;
    }
    Ok(())
}

fn add_direct_step_routes(
    document: &Document,
    routes: &mut Vec<Route<'static>>,
    steps_prefix: &[Component<'static>],
    steps: &[StepSummary],
) -> Result<(), QueryError> {
    for (index, step) in steps.iter().enumerate() {
        if step.uses == DirectUses::Missing && !step.has_merge {
            continue;
        }
        let mut step_prefix = steps_prefix.to_vec();
        step_prefix.push(index.into());
        add_uses_for_summary(document, routes, &step_prefix, step.uses, step.has_merge)?;
    }
    Ok(())
}

fn add_uses_for_summary(
    document: &Document,
    routes: &mut Vec<Route<'static>>,
    prefix: &[Component<'static>],
    uses: DirectUses,
    has_merge: bool,
) -> Result<(), QueryError> {
    match uses {
        DirectUses::StringScalar => {
            let mut path = prefix.to_vec();
            path.push("uses".into());
            routes.push(Route::from(path));
        }
        DirectUses::NonString => {}
        DirectUses::Missing if has_merge => {
            if let Some(route) = find_uses_route(document, prefix)? {
                routes.push(route);
            }
        }
        DirectUses::Missing => {}
    }
    Ok(())
}

fn find_uses_route(
    document: &Document,
    prefix: &[Component<'static>],
) -> Result<Option<Route<'static>>, QueryError> {
    let Some(path) = find_key_path(document, prefix, "uses")? else {
        return Ok(None);
    };
    Ok(matches!(node_shape(document, &path)?.0, NodeShape::Scalar).then(|| Route::from(path)))
}

fn find_key_path(
    document: &Document,
    prefix: &[Component<'static>],
    key: &'static str,
) -> Result<Option<Vec<Component<'static>>>, QueryError> {
    let mut visited_depth = HashMap::new();
    lookup_key_path(document, prefix, key, 0, &mut visited_depth)
}

fn lookup_key_path(
    document: &Document,
    prefix: &[Component<'static>],
    key: &'static str,
    depth: usize,
    visited_depth: &mut HashMap<(usize, usize), usize>,
) -> Result<Option<Vec<Component<'static>>>, QueryError> {
    let (shape, Some(span)) = node_shape(document, prefix)? else {
        return Ok(None);
    };
    if !matches!(shape, NodeShape::Mapping)
        || visited_depth
            .get(&span)
            .is_some_and(|&previous| previous <= depth)
    {
        return Ok(None);
    }
    visited_depth.insert(span, depth);
    let mut direct = prefix.to_vec();
    direct.push(key.into());
    match direct_key_status(document, &direct)? {
        DirectKey::PresentWithValue => return Ok(Some(direct)),
        DirectKey::PresentEmpty => return Ok(None),
        DirectKey::Missing => {}
    }
    if depth >= MAX_MERGE_DEPTH {
        return Ok(None);
    }
    lookup_merged_key_path(document, prefix, key, depth + 1, visited_depth)
}

fn lookup_merged_key_path(
    document: &Document,
    prefix: &[Component<'static>],
    key: &'static str,
    depth: usize,
    visited_depth: &mut HashMap<(usize, usize), usize>,
) -> Result<Option<Vec<Component<'static>>>, QueryError> {
    let mut merge_prefix = prefix.to_vec();
    merge_prefix.push("<<".into());
    match node_shape(document, &merge_prefix)?.0 {
        NodeShape::Mapping => lookup_key_path(document, &merge_prefix, key, depth, visited_depth),
        NodeShape::Sequence => {
            lookup_merged_sequence_path(document, &merge_prefix, key, depth, visited_depth)
        }
        NodeShape::Scalar | NodeShape::Absent => Ok(None),
    }
}

fn lookup_merged_sequence_path(
    document: &Document,
    merge_prefix: &[Component<'static>],
    key: &'static str,
    depth: usize,
    visited_depth: &mut HashMap<(usize, usize), usize>,
) -> Result<Option<Vec<Component<'static>>>, QueryError> {
    for index in 0.. {
        let mut item_prefix = merge_prefix.to_vec();
        item_prefix.push(index.into());
        if !sequence_has_item(document, &item_prefix)? {
            return Ok(None);
        }
        if let Some(found) = lookup_key_path(document, &item_prefix, key, depth, visited_depth)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn add_merged_step_routes(
    document: &Document,
    routes: &mut Vec<Route<'static>>,
    steps_prefix: &[Component<'static>],
) -> Result<(), QueryError> {
    if !matches!(node_shape(document, steps_prefix)?.0, NodeShape::Sequence) {
        return Ok(());
    }
    for index in 0.. {
        let mut step_prefix = steps_prefix.to_vec();
        step_prefix.push(index.into());
        if !sequence_has_item(document, &step_prefix)? {
            break;
        }
        if let Some(route) = find_uses_route(document, &step_prefix)? {
            routes.push(route);
        }
    }
    Ok(())
}

enum NodeShape {
    Mapping,
    Sequence,
    Scalar,
    Absent,
}

enum DirectKey {
    PresentWithValue,
    PresentEmpty,
    Missing,
}

fn direct_key_status(
    document: &Document,
    direct_route: &[Component<'static>],
) -> Result<DirectKey, QueryError> {
    match document.query_exact(&Route::from(direct_route.to_vec())) {
        Ok(Some(_)) => Ok(DirectKey::PresentWithValue),
        Ok(None) => Ok(DirectKey::PresentEmpty),
        Err(QueryError::ExhaustedMapping(_)) => Ok(DirectKey::Missing),
        Err(
            QueryError::UnexpectedNode(_) | QueryError::MissingChild(_, _) | QueryError::Other(_),
        ) => Ok(DirectKey::PresentEmpty),
        Err(error) => Err(error),
    }
}

fn sequence_has_item(
    document: &Document,
    item_prefix: &[Component<'static>],
) -> Result<bool, QueryError> {
    match document.query_exact(&Route::from(item_prefix.to_vec())) {
        Ok(_) => Ok(true),
        Err(
            QueryError::ExhaustedList(_, _)
            | QueryError::ExpectedList(_)
            | QueryError::UnexpectedNode(_)
            | QueryError::MissingChild(_, _)
            | QueryError::Other(_),
        ) => Ok(false),
        Err(error) => Err(error),
    }
}

fn node_shape(
    document: &Document,
    prefix: &[Component<'static>],
) -> Result<(NodeShape, Option<(usize, usize)>), QueryError> {
    let feature = match document.query_exact(&Route::from(prefix.to_vec())) {
        Ok(Some(feature)) => feature,
        Ok(None)
        | Err(
            QueryError::ExhaustedMapping(_)
            | QueryError::ExhaustedList(_, _)
            | QueryError::ExpectedMapping(_)
            | QueryError::ExpectedList(_)
            | QueryError::UnexpectedNode(_)
            | QueryError::MissingChild(_, _)
            | QueryError::Other(_),
        ) => return Ok((NodeShape::Absent, None)),
        Err(error) => return Err(error),
    };
    let span = Some(feature.location.byte_span);
    let mut probe = prefix.to_vec();
    probe.push(0usize.into());
    match document.query_exact(&Route::from(probe)) {
        Ok(_) | Err(QueryError::ExhaustedList(_, _) | QueryError::MissingChild(_, _)) => {
            Ok((NodeShape::Sequence, span))
        }
        Err(QueryError::ExpectedList(_)) => Ok((NodeShape::Mapping, span)),
        Err(QueryError::UnexpectedNode(_)) => Ok((NodeShape::Scalar, span)),
        Err(QueryError::Other(_)) => Ok((NodeShape::Absent, None)),
        Err(error) => Err(error),
    }
}
