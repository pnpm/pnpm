use wax::{
    Glob,
    Program,
};

pub(crate) struct OutputGlobs<'a> {
    pub(super) include: Vec<Glob<'a>>,
    pub(super) exclude: Vec<Glob<'a>>,
}

impl<'a> OutputGlobs<'a> {
    pub(crate) fn new(patterns: &'a [String]) -> miette::Result<Self> {
        let mut include = Vec::new();
        let mut exclude = Vec::new();
        for pattern in patterns {
            let (body, target) = match pattern.strip_prefix('!') {
                Some(body) => (body, &mut exclude),
                None => (pattern.as_str(), &mut include),
            };
            let glob = Glob::new(strip_cur_dir_prefixes(body))
                .map_err(|error| miette::miette!("invalid glob {pattern:?}: {error}"))?;
            if !matches!(glob.has_root(), wax::query::When::Never) {
                return Err(miette::miette!("cache glob must be relative: {pattern:?}"));
            }
            target.push(glob);
        }
        Ok(Self { include, exclude })
    }

    pub(super) fn is_match(&self, path: &str) -> bool {
        self.include
            .iter()
            .any(|glob| glob.is_match(path))
            && !self.exclude
                .iter()
                .any(|glob| glob.is_match(path))
    }
}

pub(super) fn strip_cur_dir_prefixes(mut pattern: &str) -> &str {
    while let Some(tail) = pattern.strip_prefix("./") {
        pattern = tail;
    }
    pattern
}
