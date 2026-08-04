import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { indexProjectRefs, toProjectDir } from '@pnpm/releasing.versioning'
import type { Project, ProjectsGraph, VersioningSettings } from '@pnpm/types'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { renderHelp } from 'render-help'

import { getReleasableProjects } from '../change/index.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    recursive: Boolean,
  }
}

export const commandNames = ['lane']

/**
 * The reserved name of the default lane: every package is on it unless
 * assigned elsewhere, packages on it release stable versions, and no
 * prerelease lane can take the name.
 */
export const MAIN_LANE = 'main'

export function help (): string {
  return renderHelp({
    description: 'Manages per-package release lanes. A lane is a parallel release track: while a package is on one, the bare "pnpm version -r" releases it as X.Y.Z-<lane>.N prereleases while the rest of the workspace keeps releasing stable versions. Moving a package back to the main lane releases its accumulated stable version on the next run. Membership lives under the versioning.lanes key of pnpm-workspace.yaml; this command is a convenience editor for that key.',
    usages: [
      'pnpm lane',
      'pnpm lane <name> --filter <pattern>',
      'pnpm lane main --filter <pattern>',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Select the packages to move between lanes',
            name: '--filter <pattern>',
          },
        ],
      },
    ],
  })
}

export type LaneCommandOptions = Pick<Config,
| 'dir'
| 'filter'
| 'versioning'
| 'workspaceDir'
> & {
  allProjects?: Project[]
  selectedProjectsGraph?: ProjectsGraph
}

export async function handler (opts: LaneCommandOptions, params: string[]): Promise<string> {
  const workspaceDir = opts.workspaceDir
  if (!workspaceDir) {
    throw new PnpmError('WORKSPACE_ONLY', 'pnpm lane is only supported in a workspace')
  }

  if (params.length === 0) {
    return renderLanes(opts.versioning?.lanes ?? {})
  }
  const laneName = params[0]

  if ((opts.filter ?? []).length === 0) {
    throw new PnpmError('VERSIONING_LANE_FILTER_REQUIRED', 'Select the packages to move with --filter, e.g. "pnpm lane alpha --filter <pkg>..."')
  }
  const refs = indexProjectRefs(opts.allProjects ?? [], workspaceDir)
  const releasableDirs = new Set(getReleasableProjects(opts.allProjects ?? [], workspaceDir, opts.versioning).map((project) => project.dir))
  const selected = Object.values(opts.selectedProjectsGraph ?? {})
    .map((node) => ({
      name: node.package.manifest.name,
      dir: toProjectDir(workspaceDir, node.package.rootDir),
    }))
    .filter((project): project is { name: string, dir: string } =>
      project.name != null && releasableDirs.has(project.dir))
  if (selected.length === 0) {
    throw new PnpmError('VERSIONING_NO_PACKAGES', 'The filter selected no releasable packages')
  }

  // Existing entries may reference projects by name or by directory; resolve
  // them so assignments and removals key on the project, not the spelling.
  const lanes = { ...opts.versioning?.lanes }
  const laneByDir = new Map<string, { key: string, lane: string }>()
  for (const [key, lane] of Object.entries(lanes)) {
    for (const dir of refs.refToDirs(key)) {
      laneByDir.set(dir, { key, lane })
    }
  }

  let output: string
  if (laneName === MAIN_LANE) {
    for (const project of selected) {
      const existing = laneByDir.get(project.dir)
      if (existing != null) {
        delete lanes[existing.key]
      }
    }
    output = `Moved to the main lane:\n${selected.map((project) => `  ${refFor(project, refs)}\n`).join('')}` +
      'The accumulated stable versions release on the next "pnpm version -r" run.'
  } else {
    if (laneName.toLowerCase() === MAIN_LANE) {
      throw new PnpmError('VERSIONING_INVALID_LANE_NAME', `Invalid lane name: ${laneName}. "main" is the reserved default lane; spell it in lowercase to move packages back onto it.`)
    }
    // A purely numeric lane name is rejected because semver parses an
    // all-digit prerelease identifier as a number, which changes sorting
    // semantics.
    if (!/^[0-9A-Z-]+$/i.test(laneName) || /^\d+$/.test(laneName)) {
      throw new PnpmError('VERSIONING_INVALID_LANE_NAME', `Invalid lane name: ${laneName}. Lane names may contain only alphanumerics and hyphens, and cannot be purely numeric.`)
    }
    for (const project of selected) {
      const existing = laneByDir.get(project.dir)
      if (existing != null && existing.lane !== laneName) {
        throw new PnpmError('VERSIONING_ALREADY_ON_LANE', `${refFor(project, refs)} is already on the "${existing.lane}" lane. Move it back with "pnpm lane main" first.`)
      }
      if (existing == null) {
        lanes[refFor(project, refs)] = laneName
      }
    }
    output = `Moved to the "${laneName}" lane:\n${selected.map((project) => `  ${refFor(project, refs)}\n`).join('')}`
  }

  const versioning: VersioningSettings = { ...opts.versioning }
  if (Object.keys(lanes).length > 0) {
    versioning.lanes = lanes
  } else {
    delete versioning.lanes
  }
  await updateWorkspaceManifest(workspaceDir, {
    updatedFields: {
      versioning: Object.keys(versioning).length > 0 ? versioning : undefined,
    },
  })
  return output
}

/**
 * How the project is referenced in versioning.lanes and in output: the bare
 * name, or the directory path when the name is shared by several projects.
 */
function refFor (project: { name: string, dir: string }, refs: { nameToDirs: (name: string) => string[] }): string {
  return refs.nameToDirs(project.name).length > 1 ? `./${project.dir}` : project.name
}

function renderLanes (lanes: Record<string, string>): string {
  const laneEntries = Object.entries(lanes)
  if (laneEntries.length === 0) {
    return 'All packages are on the main lane.'
  }
  const byLane = new Map<string, string[]>()
  for (const [ref, laneName] of laneEntries) {
    let members = byLane.get(laneName)
    if (members == null) {
      members = []
      byLane.set(laneName, members)
    }
    members.push(ref)
  }
  let output = 'Lanes:\n'
  for (const [laneName, members] of Array.from(byLane.entries()).sort(([left], [right]) => left.localeCompare(right))) {
    output += `  ${laneName}:\n`
    for (const ref of members.sort()) {
      output += `    ${ref}\n`
    }
  }
  return output
}

export const lane = {
  handler,
  help,
  commandNames,
  cliOptionsTypes,
  rcOptionsTypes,
}
