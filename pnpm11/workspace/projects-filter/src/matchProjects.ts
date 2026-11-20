import { createMatcher } from '@pnpm/config.matcher'
import type { ProjectRootDir } from '@pnpm/types'
import type { BaseProject } from '@pnpm/workspace.projects-graph'
import { isSubdir } from 'is-subdir'
import * as micromatch from 'micromatch'

import { formatDirGlob, formatDirGlobCandidate } from './dirGlob.js'
import type { ProjectGraph } from './index.js'

export function matchProjects<Pkg extends BaseProject> (
  graph: ProjectGraph<Pkg>,
  pattern: string
): ProjectRootDir[] {
  const match = createMatcher(pattern)
  const matches = (Object.keys(graph) as ProjectRootDir[]).filter((id) => graph[id].package.manifest.name && match(graph[id].package.manifest.name!))
  if (matches.length === 0 && !(pattern[0] === '@') && !pattern.includes('/')) {
    const scopedMatches = matchProjects(graph, `@*/${pattern}`)
    return scopedMatches.length !== 1 ? [] : scopedMatches
  }
  return matches
}

export function matchProjectsByExactPath<Pkg extends BaseProject> (
  graph: ProjectGraph<Pkg>,
  pathStartsWith: string
): ProjectRootDir[] {
  return (Object.keys(graph) as ProjectRootDir[]).filter((parentDir) => isSubdir(pathStartsWith, parentDir))
}

export function matchProjectsByGlob<Pkg extends BaseProject> (
  graph: ProjectGraph<Pkg>,
  pathStartsWith: string
): ProjectRootDir[] {
  const format = (str: string) => str.replace(/\/$/, '')
  const formattedFilter = formatDirGlob(pathStartsWith)
  return (Object.keys(graph) as ProjectRootDir[]).filter((parentDir) => micromatch.default.isMatch(formatDirGlobCandidate(parentDir), formattedFilter, { format }))
}
