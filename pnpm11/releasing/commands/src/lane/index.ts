import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import type { Project, ProjectsGraph, VersioningSettings } from '@pnpm/types'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { renderHelp } from 'render-help'

import { getReleasablePkgNames } from '../change/index.js'

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
  const releasable = new Set(getReleasablePkgNames(opts.allProjects ?? [], opts.versioning))
  const selected = selectedPkgNames(opts.selectedProjectsGraph ?? {}).filter((name) => releasable.has(name))
  if (selected.length === 0) {
    throw new PnpmError('VERSIONING_NO_PACKAGES', 'The filter selected no releasable packages')
  }

  const lanes = { ...opts.versioning?.lanes }
  let output: string
  if (laneName === MAIN_LANE) {
    for (const name of selected) {
      delete lanes[name]
    }
    output = `Moved to the main lane:\n${selected.map((name) => `  ${name}\n`).join('')}` +
      'The accumulated stable versions release on the next "pnpm version -r" run.'
  } else {
    // A purely numeric lane name is rejected because semver parses an
    // all-digit prerelease identifier as a number, which changes sorting
    // semantics.
    if (!/^[0-9A-Z-]+$/i.test(laneName) || /^\d+$/.test(laneName)) {
      throw new PnpmError('VERSIONING_INVALID_LANE_NAME', `Invalid lane name: ${laneName}. Lane names may contain only alphanumerics and hyphens, and cannot be purely numeric.`)
    }
    for (const name of selected) {
      if (lanes[name] != null && lanes[name] !== laneName) {
        throw new PnpmError('VERSIONING_ALREADY_ON_LANE', `${name} is already on the "${lanes[name]}" lane. Move it back with "pnpm lane main" first.`)
      }
      lanes[name] = laneName
    }
    output = `Moved to the "${laneName}" lane:\n${selected.map((name) => `  ${name}\n`).join('')}`
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

function renderLanes (lanes: Record<string, string>): string {
  const laneEntries = Object.entries(lanes)
  if (laneEntries.length === 0) {
    return 'All packages are on the main lane.'
  }
  const byLane = new Map<string, string[]>()
  for (const [pkgName, laneName] of laneEntries) {
    let members = byLane.get(laneName)
    if (members == null) {
      members = []
      byLane.set(laneName, members)
    }
    members.push(pkgName)
  }
  let output = 'Lanes:\n'
  for (const [laneName, members] of Array.from(byLane.entries()).sort(([left], [right]) => left.localeCompare(right))) {
    output += `  ${laneName}:\n`
    for (const pkgName of members.sort()) {
      output += `    ${pkgName}\n`
    }
  }
  return output
}

function selectedPkgNames (selectedProjectsGraph: ProjectsGraph): string[] {
  return Object.values(selectedProjectsGraph)
    .map((node) => node.package.manifest.name)
    .filter((name): name is string => name != null)
}

export const lane = {
  handler,
  help,
  commandNames,
  cliOptionsTypes,
  rcOptionsTypes,
}
