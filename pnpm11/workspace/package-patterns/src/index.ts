import path from 'node:path'

import micromatch from 'micromatch'

/** Directory names the workspace walk never descends into. */
const WORKSPACE_IGNORED_DIRS = new Set(['node_modules', 'bower_components'])

/**
 * Turn the `packages` patterns of `pnpm-workspace.yaml`, which select
 * directories, into the globs that select the manifests inside them.
 */
export function normalizePatterns (patterns: readonly string[]): string[] {
  const normalizedPatterns: string[] = []
  for (const pattern of patterns) {
    normalizedPatterns.push(pattern.replace(/\/?$/, '/package.{json,yaml,json5}'))
  }
  return normalizedPatterns
}

export interface IsWorkspaceProjectDirOptions {
  workspaceDir: string
  dir: string
  patterns?: string[]
}

/**
 * Tells for a single directory what walking the whole workspace tells: whether
 * the workspace at `workspaceDir` contains a project in `dir`. Both expand
 * `patterns` through {@link normalizePatterns}, so a directory this accepts is
 * a directory the walk returns.
 *
 * The workspace root is always a project (https://github.com/pnpm/pnpm/issues/1986).
 *
 * `dir` is the workspace root or a directory below it, which is what a
 * workspace lookup has in hand. A `packages` pattern may also reach above the
 * root, and the walk does return such a project, but nothing asks about one
 * here.
 */
export function isWorkspaceProjectDir ({ workspaceDir, dir, patterns }: IsWorkspaceProjectDirOptions): boolean {
  const relativeDir = path.relative(workspaceDir, dir).split(path.sep).join('/')
  if (relativeDir === '' || relativeDir === '.') return true
  if (relativeDir.split('/').some((segment) => WORKSPACE_IGNORED_DIRS.has(segment))) return false
  // Any manifest basename would do. The globs all end in the same
  // `package.{json,yaml,json5}` group, so they cannot tell one from another.
  const manifestPath = `${relativeDir}/package.json`
  const allPatterns = patterns ?? ['.', '**']
  const { included } = splitPatterns(allPatterns)
  return micromatch.isMatch(manifestPath, normalizePatterns(included)) &&
    !createManifestExclusionMatcher(allPatterns)(manifestPath)
}

/**
 * Build the test for whether a negated `packages` pattern excludes a manifest,
 * given as a path relative to the workspace root.
 *
 * A wildcard in an exclusion also matches directories whose names start with
 * a dot, so `!packages/**` leaves out `packages/.dev/tool` even when an
 * include pattern names `.dev` explicitly.
 */
export function createManifestExclusionMatcher (patterns: readonly string[]): (manifestPath: string) => boolean {
  const { excluded } = splitPatterns(patterns)
  if (excluded.length === 0) return () => false
  const globs = normalizePatterns(excluded)
  return (manifestPath) => micromatch.isMatch(manifestPath, globs, { dot: true })
}

function splitPatterns (patterns: readonly string[]): { included: string[], excluded: string[] } {
  const included: string[] = []
  const excluded: string[] = []
  for (const pattern of patterns) {
    const negated = pattern.startsWith('!')
    const body = negated ? pattern.slice(1) : pattern
    // An absolute pattern selects nothing, as the paths it is matched against
    // are relative to the workspace root. A negated one excludes nothing for
    // the same reason, so it may not reach the exclusion list.
    if (body.startsWith('/')) continue
    // The walk resolves "./" and "a/../b" on its way down. Matching a path
    // against the glob instead leaves that to be done up front.
    ;(negated ? excluded : included).push(path.posix.normalize(body))
  }
  return { included, excluded }
}
