import path from 'node:path'

import { readProjectManifest } from '@pnpm/cli.utils'
import { binDirOf, type Config, createProjectModulesDirResolver, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { makeProjectNodePathOption, runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import { isGitRepo, isWorkingTreeClean } from '@pnpm/network.git-utils'
import {
  type AppliedRelease,
  applyReleasePlan,
  type ApplyReleasePlanOptions,
  assembleReleasePlan,
  changelogStorage,
  privateProjectDirs,
  readChangeIntents,
  readLedger,
  type ReleasePlan,
  toProjectDir,
} from '@pnpm/releasing.versioning'
import type { Project, ProjectsGraph } from '@pnpm/types'
import { safeExeca as execa } from 'execa'
import { pick } from 'ramda'
import { inc, valid } from 'semver'

import { renderReleasePlan, toWorkspaceProjects } from '../change/index.js'
import { changelogHasSection, fetchPublishedChangelog } from '../publish/previousChangelog.js'
import { publishedNameByManifestName } from '../publishedNames.js'
import { type CheckVersionPublished, resolveUnpublishedDirs } from '../resolveUnpublishedDirs.js'
import { help } from './help.js'

export { help }

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
  checkVersionPublished?: CheckVersionPublished
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
  const explicitVersion = await resolveExplicitVersion(rawBump, { cwd: gitCwd, tagVersionPrefix: opts.tagVersionPrefix })
  await assertCleanWorkingTree(opts, gitCwd)

  const lifecycleOpts = { ...opts, modulesDirFor: createProjectModulesDirResolver(opts) }
  const changes = await bumpSelectedPackageVersions({ rawBump, explicitVersion, opts: lifecycleOpts })

  if (changes.length === 0) {
    throw new PnpmError('NO_PACKAGES_TO_VERSION', 'No packages to version')
  }

  // In recursive mode, multiple packages can be bumped to different versions
  // in a single run, and there is no obvious single version to tag the commit
  // with. Skip the git commit and tag entirely in that case.
  if (!opts.dryRun && !opts.recursive && opts.gitTagVersion !== false && await isGitRepo({ cwd: gitCwd })) {
    await commitAndTag(changes, { ...opts, cwd: gitCwd })
  }

  await Promise.all(changes.map(change => runVersionLifecycleHook('postversion', change, lifecycleOpts)))

  return renderVersionChanges(changes, opts)
}

/**
 * The exact version `rawBump` names, or `null` when it is a bump type.
 * Throws when it is neither.
 */
async function resolveExplicitVersion (
  rawBump: string,
  { cwd, tagVersionPrefix }: { cwd: string, tagVersionPrefix?: string }
): Promise<string | null> {
  const explicitVersion = rawBump === 'from-git'
    ? await versionFromGit(cwd, tagVersionPrefix)
    : valid(rawBump)
  if (!explicitVersion && !isBumpType(rawBump)) {
    throw new PnpmError('INVALID_VERSION_BUMP', `Invalid version argument: ${rawBump}. Must be a valid semver version (e.g. 1.2.3) or one of: major, minor, patch, premajor, preminor, prepatch, prerelease, from-git`)
  }
  return explicitVersion
}

async function assertCleanWorkingTree (opts: VersionHandlerOptions, cwd: string): Promise<void> {
  if (opts.dryRun || opts.gitChecks === false || !await isGitRepo({ cwd })) return
  if (!await isWorkingTreeClean({ cwd })) {
    throw new PnpmError('UNCLEAN_WORKING_TREE', 'Working tree is not clean. Commit or stash your changes.')
  }
}

async function bumpSelectedPackageVersions ({ rawBump, explicitVersion, opts }: Omit<BumpPackageVersionOptions, 'pkgDir'>): Promise<VersionChange[]> {
  const pkgDirs = opts.recursive ? Object.keys(opts.selectedProjectsGraph ?? {}) : [opts.dir]
  const bumpResults = await Promise.all(
    pkgDirs.map(pkgDir => bumpPackageVersion({ pkgDir, rawBump, explicitVersion, opts }))
  )
  return bumpResults.filter((change): change is VersionChange => change != null)
}

function renderVersionChanges (changes: VersionChange[], opts: Pick<VersionHandlerOptions, 'dryRun' | 'json'>): string {
  if (opts.json) {
    return JSON.stringify(changes.map(({ manifestPath: _manifestPath, ...change }) => change), null, 2)
  }

  let output = opts.dryRun ? 'Version bump plan:\n' : 'Version bumped successfully:\n'
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

  await assertCleanWorkingTree(opts, workspaceDir)

  const { applyOpts, filter, plan } = await planReleaseFromIntents(opts, workspaceDir)

  if (plan.releases.length === 0) {
    // A full (unfiltered) run garbage-collects the intent files an empty plan
    // leaves behind: declined ("none"-only) intents and files a merge
    // resurrected after every named package had already consumed them. A
    // filtered run must not — "nothing pending in this scope" is no reason to
    // delete prose belonging to packages outside the filter.
    if (!opts.dryRun && filter == null) {
      await applyReleasePlan(plan, applyOpts)
    }
    return opts.json ? '[]' : 'No pending changes. Record one with "pnpm change".'
  }

  if (opts.dryRun) {
    return renderReleasePlan(plan)
  }

  return renderAppliedReleases(await applyReleasePlan(plan, applyOpts), opts.json)
}

interface IntentReleasePlan {
  applyOpts: ApplyReleasePlanOptions
  filter?: Set<string>
  plan: ReleasePlan
}

async function planReleaseFromIntents (opts: VersionHandlerOptions, workspaceDir: string): Promise<IntentReleasePlan> {
  const intents = await readChangeIntents(workspaceDir)
  const ledger = await readLedger(workspaceDir)
  const projects = toWorkspaceProjects(opts.allProjects ?? [])
  const filter = (opts.filter ?? []).length > 0
    ? new Set(Object.keys(opts.selectedProjectsGraph ?? {}).map((rootDir) => toProjectDir(workspaceDir, rootDir)))
    : undefined

  const baseArgs = {
    workspaceDir,
    projects,
    intents,
    ledger,
    versioning: opts.versioning,
    filter,
    enforceWorkspaceProtocol: true,
  }
  const publishedNames = publishedNameByManifestName(projects)
  const privateDirs = privateProjectDirs(projects, workspaceDir)
  const unpublishedDirs = await resolveUnpublishedDirs(assembleReleasePlan(baseArgs), { ...opts, publishedNames, privateDirs })
  const plan = assembleReleasePlan({ ...baseArgs, unpublishedDirs })

  const applyOpts: ApplyReleasePlanOptions = {
    workspaceDir,
    projects,
    allIntents: intents,
    versioning: opts.versioning,
    verifyPublished: buildVerifyPublished(opts, publishedNames),
  }
  return { applyOpts, filter, plan }
}

function renderAppliedReleases (applied: AppliedRelease[], json: boolean | undefined): string {
  if (json) {
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
function buildVerifyPublished (opts: VersionHandlerOptions, publishedNames: ReadonlyMap<string, string>): ApplyReleasePlanOptions['verifyPublished'] {
  if (changelogStorage(opts.versioning) !== 'registry') return undefined
  return async (name, version, section) => {
    try {
      // The parked section is keyed by the manifest name, which is what the
      // ledger joins on; the registry only knows the published one.
      const changelog = await fetchPublishedChangelog(opts, publishedNames.get(name) ?? name, version)
      return changelog != null && changelogHasSection(changelog, section)
    } catch {
      return false
    }
  }
}

function invalidVersionFromGitError (cwd: string, tagVersionPrefix: string, reason: string): PnpmError {
  return new PnpmError('INVALID_VERSION_FROM_GIT', `Could not determine a valid version from Git in ${JSON.stringify(cwd)} using tag prefix ${JSON.stringify(tagVersionPrefix)}: ${reason}`)
}

async function versionFromGit (cwd: string, tagVersionPrefix = 'v'): Promise<string> {
  const { stdout } = await execa('git', ['describe', '--tags', '--abbrev=0', '--always', '--match=' + tagVersionPrefix + '*.*.*'], { cwd })
  const tag = typeof stdout === 'string' ? stdout.trim() : ''
  const { stdout: matchingTag } = await execa('git', ['tag', '--list', '--', tag], { cwd })

  if (typeof matchingTag !== 'string' || matchingTag.trim() !== tag) {
    throw invalidVersionFromGitError(cwd, tagVersionPrefix, 'no matching Git tag found')
  }

  const version = tag.startsWith(tagVersionPrefix)
    ? valid(tag.slice(tagVersionPrefix.length))
    : null
  if (!version) {
    throw invalidVersionFromGitError(cwd, tagVersionPrefix, 'tag is not a valid version: ' + JSON.stringify(tag))
  }
  return version
}

type VersionLifecycleOptions = VersionHandlerOptions & { modulesDirFor: ReturnType<typeof createProjectModulesDirResolver> }

interface BumpPackageVersionOptions {
  pkgDir: string
  rawBump: string
  explicitVersion: string | null
  opts: VersionLifecycleOptions
}

async function bumpPackageVersion ({ pkgDir, rawBump, explicitVersion, opts }: BumpPackageVersionOptions): Promise<VersionChange | null> {
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
  if (!opts.dryRun) {
    await writeProjectManifest(manifest)
  }

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

async function runVersionLifecycleHook (stage: 'preversion' | 'version' | 'postversion', change: VersionChange, opts: VersionLifecycleOptions): Promise<void> {
  if (opts.ignoreScripts === true || opts.dryRun) return

  const { manifest } = await readProjectManifest(change.path)
  const wdBinDir = binDirOf(change.path, opts.modulesDirFor(manifest.name))
  const lifecycleOpts: RunLifecycleHookOptions = {
    depPath: change.name,
    wdBinDir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: { ...opts.extraEnv, ...await makeProjectNodePathOption({ modulesDir: path.dirname(wdBinDir), rootDir: change.path }, opts) },
    initCwd: opts.dir,
    pkgRoot: change.path,
    rootModulesDir: path.dirname(wdBinDir),
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
