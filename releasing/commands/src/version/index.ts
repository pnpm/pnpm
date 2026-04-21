import path from 'node:path'

import { readProjectManifest } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { isGitRepo, isWorkingTreeClean } from '@pnpm/network.git-utils'
import { filterProjectsFromDir, type WorkspaceFilter } from '@pnpm/workspace.projects-filter'
import { safeExeca as execa } from 'execa'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import { inc, valid } from 'semver'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'allow-same-version',
    'commit-hooks',
    'git-checks',
    'git-tag-version',
    'message',
    'sign-git-tag',
    'tag-version-prefix',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    json: Boolean,
    preid: String,
    recursive: Boolean,
  }
}

export const commandNames = ['version']

const BUMP_TYPES = ['major', 'minor', 'patch', 'premajor', 'preminor', 'prepatch', 'prerelease'] as const
type BumpType = typeof BUMP_TYPES[number]

function isBumpType (value: string): value is BumpType {
  return (BUMP_TYPES as readonly string[]).includes(value)
}

export function help (): string {
  return renderHelp({
    description: 'Bumps the version of a package.',
    usages: [
      'pnpm version <newversion>',
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
            description: 'Sets the tag prefix. Default is "v". Set to empty string to remove the prefix.',
            name: '--tag-version-prefix <prefix>',
          },
          {
            description: 'Allow bumping to the same version',
            name: '--allow-same-version',
          },
          {
            description: 'Commit message. "%s" is replaced with the new version. Default is "%s".',
            name: '--message <message>',
          },
          {
            description: "Don't create a commit or tag for the version bump. Git commits and tags are always skipped in recursive mode.",
            name: '--no-git-tag-version',
          },
          {
            description: 'Skip running git commit hooks when committing the version bump',
            name: '--no-commit-hooks',
          },
          {
            description: 'Sign the generated git tag with GPG',
            name: '--sign-git-tag',
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
  manifestPath: string
}

interface VersionHandlerOptions extends Config {
  allowSameVersion?: boolean
  commitHooks?: boolean
  gitChecks?: boolean
  gitTagVersion?: boolean
  json?: boolean
  message?: string
  preid?: string
  recursive?: boolean
  signGitTag?: boolean
  tagVersionPrefix?: string
}

export async function handler (
  opts: VersionHandlerOptions,
  params: string[]
): Promise<string | { output?: string, exitCode: number }> {
  const rawBump = params[0]

  if (!rawBump) {
    throw new PnpmError('INVALID_VERSION_BUMP', 'A version argument is required. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease')
  }

  const explicitVersion = valid(rawBump)
  if (!explicitVersion && !isBumpType(rawBump)) {
    throw new PnpmError('INVALID_VERSION_BUMP', `Invalid version argument: ${rawBump}. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease`)
  }

  const gitCwd = opts.workspaceDir ?? opts.dir

  if (opts.gitChecks !== false && await isGitRepo({ cwd: gitCwd })) {
    if (!await isWorkingTreeClean({ cwd: gitCwd })) {
      throw new PnpmError('UNCLEAN_WORKING_TREE', 'Working tree is not clean. Commit or stash your changes.')
    }
  }

  const changes: VersionChange[] = []

  if (opts.recursive) {
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
      pkgDirs.map(pkgDir => bumpPackageVersion(pkgDir, rawBump, explicitVersion, opts))
    )
    for (const change of bumpResults) {
      if (change) {
        changes.push(change)
      }
    }
  } else {
    const change = await bumpPackageVersion(opts.dir, rawBump, explicitVersion, opts)
    if (change) {
      changes.push(change)
    }
  }

  if (changes.length === 0) {
    throw new PnpmError('NO_PACKAGES_TO_VERSION', 'No packages to version')
  }

  // In recursive mode, multiple packages can be bumped to different versions
  // in a single run, and there is no obvious single version to tag the commit
  // with. Skip the git commit and tag entirely in that case.
  if (!opts.recursive && opts.gitTagVersion !== false && await isGitRepo({ cwd: gitCwd })) {
    await commitAndTag(changes, { ...opts, cwd: gitCwd })
  }

  if (opts.json) {
    return JSON.stringify(changes.map(({ manifestPath: _manifestPath, ...change }) => change), null, 2)
  }

  let output = 'Version bumped successfully:\n'
  for (const change of changes) {
    output += `${change.name}: ${change.currentVersion} → ${change.newVersion}\n`
  }

  return output
}

async function bumpPackageVersion (
  pkgDir: string,
  rawBump: string,
  explicitVersion: string | null,
  opts: VersionHandlerOptions
): Promise<VersionChange | null> {
  const { manifest, writeProjectManifest, fileName } = await readProjectManifest(pkgDir)

  if (!manifest.name || !manifest.version) {
    return null
  }

  const currentVersion = manifest.version

  if (!valid(currentVersion)) {
    throw new PnpmError('INVALID_VERSION', `Invalid version in ${pkgDir}: ${currentVersion}`)
  }

  const newVersion = explicitVersion ?? inc(currentVersion, rawBump as BumpType, false, opts.preid)

  if (!newVersion) {
    throw new PnpmError('VERSION_BUMP_FAILED', `Failed to bump version from ${currentVersion} using ${rawBump}`)
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
    manifestPath: path.join(pkgDir, fileName),
  }
}

async function commitAndTag (changes: VersionChange[], opts: VersionHandlerOptions & { cwd: string }): Promise<void> {
  const resolvedCwd = path.resolve(opts.cwd)
  const [change] = changes
  const rawMessage = opts.message ?? '%s'
  const message = rawMessage.replace(/%s/g, change.newVersion)
  const tagPrefix = opts.tagVersionPrefix ?? 'v'
  const tagName = `${tagPrefix}${change.newVersion}`
  const execOpts = { cwd: opts.cwd }

  const resolvedManifestPath = path.resolve(change.manifestPath)
  const relativeManifestPath = path.relative(resolvedCwd, resolvedManifestPath)
  if (
    relativeManifestPath === '' ||
    path.isAbsolute(relativeManifestPath) ||
    relativeManifestPath.startsWith(`..${path.sep}`) ||
    relativeManifestPath === '..'
  ) {
    throw new PnpmError(
      'INVALID_MANIFEST_PATH',
      `Cannot stage manifest outside of git cwd: ${change.manifestPath}`
    )
  }
  const manifestPath = relativeManifestPath.split(path.sep).join('/')
  await execa('git', ['add', manifestPath], execOpts)

  const commitArgs = ['commit', '-m', message]
  if (opts.commitHooks === false) {
    commitArgs.push('--no-verify')
  }
  // writeProjectManifest skips writing when the new content matches the existing
  // file, so an --allow-same-version run can leave nothing staged and fail the
  // commit. Pass --allow-empty in that case to let the tag point at the current
  // HEAD as a deliberate marker.
  if (opts.allowSameVersion) {
    commitArgs.push('--allow-empty')
  }
  await execa('git', commitArgs, execOpts)

  const tagArgs = ['tag']
  if (opts.signGitTag) {
    tagArgs.push('-s')
  } else {
    tagArgs.push('-a')
  }
  tagArgs.push(tagName, '-m', message)
  await execa('git', tagArgs, execOpts)
}

export const version = {
  handler,
  help,
  commandNames,
  cliOptionsTypes,
  rcOptionsTypes,
}
