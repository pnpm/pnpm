import { readProjectManifest } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { isGitRepo, isWorkingTreeClean } from '@pnpm/network.git-utils'
import { filterProjectsFromDir, type WorkspaceFilter } from '@pnpm/workspace.projects-filter'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import { inc, valid } from 'semver'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'git-checks',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    'allow-same-version': Boolean,
    'no-git-checks': Boolean,
    'no-commit-hooks': Boolean,
    'no-strict': Boolean,
    'workspace': Boolean,
    'workspaces': Boolean,
    'preid': String,
    'tag-version-prefix': String,
    recursive: Boolean,
    json: Boolean,
  }
}

export const commandNames = ['version']

export function help (): string {
  return renderHelp({
    description: 'Bumps the version of packages in a workspace.',
    usages: [
      'pnpm version <major|minor|patch|premajor|preminor|prepatch|prerelease>',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: "Don't check if working tree is clean",
            name: '--no-git-checks',
          },
          {
            description: 'Sets the prerelease identifier (e.g. alpha, beta, rc)',
            name: '--preid <preid>',
          },
          {
            description: 'Sets the tag prefix (default: v)',
            name: '--tag-version-prefix <prefix>',
          },
          {
            description: 'Allow bumping to the same version',
            name: '--allow-same-version',
          },
          {
            description: 'Skip running commit hooks',
            name: '--no-commit-hooks',
          },
          {
            description: 'Apply to all workspace packages',
            name: '--workspace',
          },
          {
            description: 'Filter packages by name (glob pattern)',
            name: '--filter <pattern>',
          },
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
          {
            description: 'Apply command to all packages in workspace',
            name: '--recursive',
          },
        ],
      },
    ],
  })
}

interface VersionChange {
  name: string
  currentVersion: string
  newVersion: string
  path: string
}

interface VersionHandlerOptions extends Config {
  allowSameVersion?: boolean
  noGitChecks?: boolean
  noCommitHooks?: boolean
  noStrict?: boolean
  workspace?: boolean
  workspaces?: boolean
  preid?: string
  tagVersionPrefix?: string
  recursive?: boolean
  json?: boolean
  ignoredPackages?: string[]
}

export async function handler (
  opts: VersionHandlerOptions,
  params: string[]
): Promise<string | { output?: string, exitCode: number }> {
  const bumpType = params[0]

  if (!bumpType || !['major', 'minor', 'patch', 'premajor', 'preminor', 'prepatch', 'prerelease'].includes(bumpType)) {
    throw new PnpmError('INVALID_VERSION_BUMP', 'Invalid version bump type. Must be one of: major, minor, patch, premajor, preminor, prepatch, prerelease')
  }

  const isWorkspace = opts.workspace || opts.workspaces || opts.recursive || opts.workspaceRoot

  // Check git status if needed
  if (!opts.noGitChecks && await isGitRepo()) {
    if (!await isWorkingTreeClean()) {
      throw new PnpmError('UNCLEAN_WORKING_TREE', 'Working tree is not clean. Commit or stash your changes.')
    }
  }

  const changes: VersionChange[] = []

  if (isWorkspace) {
    // Handle workspace versioning
    const workspaceDir = opts.workspaceDir || opts.dir
    const filters: WorkspaceFilter[] = []

    if (opts.filter && opts.filter.length > 0) {
      opts.filter.forEach(filterPattern => {
        filters.push({
          filter: filterPattern,
          followProdDepsOnly: !!opts.filterProd && opts.filterProd.length > 0,
        })
      })
    }

    const result = await filterProjectsFromDir(
      workspaceDir,
      filters,
      {
        workspaceDir,
        prefix: opts.dir,
      }
    )

    const pkgDirs = Object.keys(result.selectedProjectsGraph)
    const bumpResults = await Promise.all(
      pkgDirs.map(pkgDir => bumpPackageVersion(pkgDir, bumpType, opts))
    )
    for (const change of bumpResults) {
      if (change) {
        changes.push(change)
      }
    }
  } else {
    // Handle single package versioning
    const change = await bumpPackageVersion(opts.dir, bumpType, opts)
    if (change) {
      changes.push(change)
    }
  }

  if (changes.length === 0) {
    throw new PnpmError('NO_PACKAGES_TO_VERSION', 'No packages to version')
  }

  // Output results
  if (opts.json) {
    return JSON.stringify(changes, null, 2)
  }

  let output = 'Version bumped successfully:\n'
  for (const change of changes) {
    output += `${change.name}: ${change.currentVersion} → ${change.newVersion}\n`
  }

  return output
}

async function bumpPackageVersion (
  pkgDir: string,
  bumpType: string,
  opts: VersionHandlerOptions
): Promise<VersionChange | null> {
  const { manifest, writeProjectManifest } = await readProjectManifest(pkgDir)

  if (!manifest.name || !manifest.version) {
    return null
  }

  const currentVersion = manifest.version

  if (!valid(currentVersion)) {
    throw new PnpmError('INVALID_VERSION', `Invalid version in ${pkgDir}: ${currentVersion}`)
  }

  const newVersion = inc(currentVersion, bumpType as 'major' | 'minor' | 'patch' | 'premajor' | 'preminor' | 'prepatch' | 'prerelease', false, opts.preid)

  if (!newVersion) {
    throw new PnpmError('VERSION_BUMP_FAILED', `Failed to bump version from ${currentVersion} using ${bumpType}`)
  }

  if (newVersion === currentVersion && !opts.allowSameVersion) {
    throw new PnpmError('VERSION_NOT_CHANGED', `Version was not changed: ${currentVersion}`)
  }

  manifest.version = newVersion
  await writeProjectManifest(manifest)

  return {
    name: manifest.name,
    currentVersion,
    newVersion,
    path: pkgDir,
  }
}

export const version = {
  handler,
  help,
  commandNames,
  cliOptionsTypes,
  rcOptionsTypes,
}
