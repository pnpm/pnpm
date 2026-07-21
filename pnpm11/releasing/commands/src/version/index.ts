import path from 'node:path'

import { readProjectManifest } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import { isGitRepo, isWorkingTreeClean } from '@pnpm/network.git-utils'
import {
  applyReleasePlan,
  type ApplyReleasePlanOptions,
  assembleReleasePlan,
  changelogStorage,
  readChangeIntents,
  readLedger,
  toProjectDir,
} from '@pnpm/releasing.versioning'
import type { Project, ProjectsGraph } from '@pnpm/types'
import { safeExeca as execa } from 'execa'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import { inc, valid } from 'semver'

import { renderReleasePlan, toWorkspaceProjects } from '../change/index.js'
import { changelogHasSection, fetchPublishedChangelog } from '../publish/previousChangelog.js'

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
    'dry-run': Boolean,
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
      'pnpm version <major|minor|patch|premajor|preminor|prepatch|prerelease|from-git>',
      'pnpm version -r [--dry-run]',
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
            description: 'Apply command to all packages in workspace. Without a version argument, consumes the pending change intents from .changeset/ and applies the resulting release plan',
            name: '--recursive',
          },
          {
            description: 'Print the release plan the pending change intents produce without applying it',
            name: '--dry-run',
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
  allProjects?: Project[]
  allowSameVersion?: boolean
  commitHooks?: boolean
  dryRun?: boolean
  gitChecks?: boolean
  gitTagVersion?: boolean
  json?: boolean
  message?: string
  preid?: string
  recursive?: boolean
  selectedProjectsGraph?: ProjectsGraph
  signGitTag?: boolean
  tagVersionPrefix?: string
}

export async function handler (
  opts: VersionHandlerOptions,
  params: string[]
): Promise<string | { output?: string, exitCode: number }> {
  const rawBump = params[0]

  if (!rawBump) {
    if (opts.recursive) {
      return releaseFromIntents(opts)
    }
    throw new PnpmError('INVALID_VERSION_BUMP', 'A version argument is required. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease, from-git')
  }

  const gitCwd = opts.workspaceDir ?? opts.dir
  const explicitVersion = rawBump === 'from-git'
    ? await versionFromGit(gitCwd, opts.tagVersionPrefix)
    : valid(rawBump)
  if (!explicitVersion && !isBumpType(rawBump)) {
    throw new PnpmError('INVALID_VERSION_BUMP', `Invalid version argument: ${rawBump}. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease, from-git`)
  }

  if (opts.gitChecks !== false && await isGitRepo({ cwd: gitCwd })) {
    if (!await isWorkingTreeClean({ cwd: gitCwd })) {
      throw new PnpmError('UNCLEAN_WORKING_TREE', 'Working tree is not clean. Commit or stash your changes.')
    }
  }

  const changes: VersionChange[] = []

  if (opts.recursive) {
    const pkgDirs = Object.keys(opts.selectedProjectsGraph ?? {})
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

  await Promise.all(changes.map(change => runVersionLifecycleHook('postversion', change, opts)))

  if (opts.json) {
    return JSON.stringify(changes.map(({ manifestPath: _manifestPath, ...change }) => change), null, 2)
  }

  let output = 'Version bumped successfully:\n'
  for (const change of changes) {
    output += `${change.name}: ${change.currentVersion} → ${change.newVersion}\n`
  }

  return output
}

async function releaseFromIntents (opts: VersionHandlerOptions): Promise<string> {
  const workspaceDir = opts.workspaceDir
  if (!workspaceDir) {
    throw new PnpmError('WORKSPACE_ONLY', 'The bare "pnpm version -r" form consumes change intents and is only supported in a workspace')
  }

  if (!opts.dryRun && opts.gitChecks !== false && await isGitRepo({ cwd: workspaceDir })) {
    if (!await isWorkingTreeClean({ cwd: workspaceDir })) {
      throw new PnpmError('UNCLEAN_WORKING_TREE', 'Working tree is not clean. Commit or stash your changes.')
    }
  }

  const intents = await readChangeIntents(workspaceDir)
  const ledger = await readLedger(workspaceDir)
  const projects = toWorkspaceProjects(opts.allProjects ?? [])
  const filter = (opts.filter ?? []).length > 0
    ? new Set(Object.keys(opts.selectedProjectsGraph ?? {}).map((rootDir) => toProjectDir(workspaceDir, rootDir)))
    : undefined

  const plan = assembleReleasePlan({
    workspaceDir,
    projects,
    intents,
    ledger,
    versioning: opts.versioning,
    filter,
    enforceWorkspaceProtocol: true,
  })

  const applyOpts: ApplyReleasePlanOptions = {
    workspaceDir,
    projects,
    allIntents: intents,
    versioning: opts.versioning,
    verifyPublished: buildVerifyPublished(opts),
  }

  if (plan.releases.length === 0) {
    // A full (unfiltered) run garbage-collects the intent files an empty plan
    // leaves behind: declined ("none"-only) intents and files a merge
    // resurrected after every named package had already consumed them. A
    // filtered run must not — "nothing pending in this scope" is no reason to
    // delete prose belonging to packages outside the filter.
    if (!opts.dryRun && filter == null) {
      await applyReleasePlan(plan, applyOpts)
    }
    return 'No pending changes. Record one with "pnpm change".'
  }

  if (opts.dryRun) {
    return renderReleasePlan(plan)
  }

  const applied = await applyReleasePlan(plan, applyOpts)

  if (opts.json) {
    return JSON.stringify(applied, null, 2)
  }
  let output = 'Versions applied:\n'
  for (const release of applied) {
    output += `${release.name}: ${release.currentVersion} → ${release.newVersion}\n`
  }
  return output
}

/**
 * In `registry` storage, the gate that lets consumed intents be collected:
 * the release must be published and its tarball's CHANGELOG.md must already
 * carry the composed section. Any error resolving that (offline, transient
 * failure) counts as "not confirmed" so the intent — still the only prose —
 * is kept. `undefined` in `repository` storage, where the committed changelog
 * makes the ledger alone sufficient.
 */
function buildVerifyPublished (opts: VersionHandlerOptions): ApplyReleasePlanOptions['verifyPublished'] {
  if (changelogStorage(opts.versioning) !== 'registry') return undefined
  return async (name, version, section) => {
    try {
      const changelog = await fetchPublishedChangelog(opts, name, version)
      return changelog != null && changelogHasSection(changelog, section)
    } catch {
      return false
    }
  }
}

async function versionFromGit (cwd: string, tagVersionPrefix = 'v'): Promise<string> {
  let tag: string
  try {
    const { stdout } = await execa('git', ['describe', '--tags', '--abbrev=0', '--match=' + tagVersionPrefix + '*.*.*'], { cwd })
    tag = typeof stdout === 'string' ? stdout.trim() : ''
  } catch {
    throw new PnpmError('INVALID_VERSION_FROM_GIT', 'No matching Git tag found in ' + JSON.stringify(cwd) + ' for prefix: ' + JSON.stringify(tagVersionPrefix))
  }
  const version = tag.startsWith(tagVersionPrefix)
    ? valid(tag.slice(tagVersionPrefix.length))
    : null
  if (!version) {
    throw new PnpmError('INVALID_VERSION_FROM_GIT', 'Tag is not a valid version: ' + JSON.stringify(tag))
  }
  return version
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

  const preVersionChange: VersionChange = {
    name: manifest.name,
    currentVersion,
    newVersion: currentVersion,
    path: pkgDir,
    manifestPath: path.join(pkgDir, fileName),
  }
  await runVersionLifecycleHook('preversion', preVersionChange, opts)

  const newVersion = explicitVersion ?? inc(currentVersion, rawBump as BumpType, false, opts.preid)

  if (!newVersion) {
    throw new PnpmError('VERSION_BUMP_FAILED', `Failed to bump version from ${currentVersion} using ${rawBump}`)
  }

  if (newVersion === currentVersion && !opts.allowSameVersion) {
    throw new PnpmError('VERSION_NOT_CHANGED', `Version was not changed: ${currentVersion}`)
  }

  manifest.version = newVersion
  await writeProjectManifest(manifest)

  const change = {
    name: manifest.name,
    currentVersion,
    newVersion,
    path: pkgDir,
    manifestPath: path.join(pkgDir, fileName),
  }
  await runVersionLifecycleHook('version', change, opts)

  return change
}

async function runVersionLifecycleHook (stage: 'preversion' | 'version' | 'postversion', change: VersionChange, opts: VersionHandlerOptions): Promise<void> {
  if (opts.ignoreScripts === true) return

  const { manifest } = await readProjectManifest(change.path)
  const lifecycleOpts: RunLifecycleHookOptions = {
    depPath: change.name,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: opts.extraEnv,
    initCwd: opts.dir,
    pkgRoot: change.path,
    rootModulesDir: path.join(change.path, opts.modulesDir ?? 'node_modules'),
    scriptShell: opts.scriptShell,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    shellEmulator: opts.shellEmulator,
    stdio: 'inherit',
    unsafePerm: opts.unsafePerm ?? false,
    userAgent: opts.userAgent,
  }
  await runLifecycleHook(stage, manifest, lifecycleOpts)
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
