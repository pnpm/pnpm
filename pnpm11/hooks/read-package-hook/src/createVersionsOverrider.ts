import path from 'node:path'

import type { PackageSelector, VersionOverride as VersionOverrideBase } from '@pnpm/config.parse-overrides'
import { isValidPeerRange } from '@pnpm/deps.peer-range'
import { barePathIsUnambiguous, isDriveLetterPrefix, isFilespec, isTarballFilename } from '@pnpm/resolving.local-resolver'
import { type Dependencies, DEPENDENCIES_OR_PEER_FIELDS, type DependenciesOrPeersField, type PackageManifest, type ProjectManifest, type ReadPackageHook } from '@pnpm/types'
import normalizePath from 'normalize-path'
import { partition } from 'ramda'
import semver from 'semver'

import { isIntersectingRange } from './isIntersectingRange.js'

export type VersionOverrideWithoutRawSelector = Omit<VersionOverrideBase, 'selector'>

export interface CreateVersionsOverriderOptions {
  /**
   * Populated with every declared semver range seen for packages that have a
   * convergence override, whether or not the override's version satisfied it.
   * Feeds the staleness check for convergence overrides after a full
   * resolution. Edges claimed by an explicit override are not recorded — the
   * convergence override never governs them.
   */
  convergeDeclaredRanges?: Map<string, Set<string>>
}

/**
 * Resolves the specifier an override imposes on a single dependency edge, or
 * `undefined` when no override claims it. `'-'` means the edge is removed.
 *
 * Edges that have no declaring manifest — a peer pnpm auto-installs — reach
 * the overrides through this function instead of through the read-package
 * hook, so parent-scoped overrides (`parent>child`) never apply to them.
 */
export type DependencyOverrider = (name: string, bareSpecifier: string, dir?: string) => string | undefined

/**
 * `undefined` when no override in the set could ever claim an undeclared
 * dependency, so the resolver skips the per-peer call in the common case of a
 * project with no overrides — or with parent-scoped ones only.
 */
export function createDependencyOverrider (
  overrides: VersionOverrideWithoutRawSelector[],
  rootDir: string
): DependencyOverrider | undefined {
  const { genericVersionOverrides, convergeVersions } = splitOverrides(overrides, rootDir)
  if (genericVersionOverrides.length === 0 && convergeVersions.size === 0) return undefined
  return (name, bareSpecifier, dir) => {
    const versionOverride = pickVersionOverride({ versionOverrides: [], genericVersionOverrides }, name, bareSpecifier)
    if (versionOverride) {
      return versionOverride.newBareSpecifier === '-'
        ? '-'
        : resolveOverriddenBareSpecifier(versionOverride, dir)
    }
    return convergeBareSpecifier(convergeVersions, name, bareSpecifier)
  }
}

/**
 * What an override needs to read to decide whether it claims a dependency: the
 * parent half of a `parent>child` key is matched against the manifest's own
 * name and version. Workspace projects may declare neither, so this is wider
 * than {@link PackageManifest}.
 */
export type OverridableManifest = Pick<ProjectManifest, 'name' | 'version' | DependenciesOrPeersField>

/**
 * Whether an override claims a dependency declared as `bareSpecifier` —
 * whether or not it rewrites the text. The specifier is part of the question:
 * a range-scoped override (`foo@^2`) claims one declaration of `foo` and not
 * another, so an alias alone cannot answer it.
 */
export type OverriddenDependencyMatcher = (alias: string, bareSpecifier: string) => boolean

/**
 * Builds an {@link OverriddenDependencyMatcher} for one manifest: a
 * `parent>child` override key is matched against that manifest's own name and
 * version, so a project asks only about the overrides that govern its own
 * declarations. Binding to the manifest also selects those once, rather than
 * once per dependency asked about.
 *
 * `undefined` when no override in the set could claim anything, so a caller
 * with none configured skips the work entirely.
 */
export function createOverriddenDependencyMatcher (
  overrides: VersionOverrideWithoutRawSelector[],
  rootDir: string
): ((manifest: OverridableManifest) => OverriddenDependencyMatcher) | undefined {
  const { versionOverrides, genericVersionOverrides, convergeVersions } = splitOverrides(overrides, rootDir)
  if (versionOverrides.length === 0 && genericVersionOverrides.length === 0 && convergeVersions.size === 0) {
    return undefined
  }
  return (manifest) => {
    const overridesForManifest = {
      versionOverrides: pickOverridesOfParent(versionOverrides, manifest),
      genericVersionOverrides,
    }
    return (alias, bareSpecifier) =>
      pickVersionOverride(overridesForManifest, alias, bareSpecifier) != null ||
      convergeBareSpecifier(convergeVersions, alias, bareSpecifier) != null
  }
}

export function createVersionsOverrider (
  overrides: VersionOverrideWithoutRawSelector[],
  rootDir: string,
  opts?: CreateVersionsOverriderOptions
): ReadPackageHook {
  const { versionOverrides, genericVersionOverrides, convergeVersions } = splitOverrides(overrides, rootDir)
  return ((manifest: PackageManifest, dir?: string) => {
    const versionOverridesWithParent = pickOverridesOfParent(versionOverrides, manifest)
    // The manifest may come from a shared cache, so the fields the overrides
    // rewrite are cloned instead of mutated in place.
    const clonedManifest = { ...manifest }
    for (const depsField of DEPENDENCIES_OR_PEER_FIELDS) {
      if (manifest[depsField] != null) {
        clonedManifest[depsField] = { ...manifest[depsField] }
      }
    }
    if (manifest.peerDependenciesMeta != null) {
      clonedManifest.peerDependenciesMeta = { ...manifest.peerDependenciesMeta }
    }
    overrideDepsOfPkg({ manifest: clonedManifest, dir }, versionOverridesWithParent, genericVersionOverrides, {
      convergeVersions,
      convergeDeclaredRanges: opts?.convergeDeclaredRanges,
    })

    return clonedManifest
  }) as ReadPackageHook
}

/**
 * The parent-scoped overrides (`parent>child`) whose parent half this manifest
 * answers to. The rest never govern its dependencies.
 */
function pickOverridesOfParent (
  versionOverrides: VersionOverrideWithParent[],
  manifest: OverridableManifest
): VersionOverrideWithParent[] {
  return versionOverrides.filter(({ parentPkg }) => (
    parentPkg.name === manifest.name &&
    (!parentPkg.bareSpecifier || (manifest.version != null && semver.satisfies(manifest.version, parentPkg.bareSpecifier)))
  ))
}

function splitOverrides (overrides: VersionOverrideWithoutRawSelector[], rootDir: string): {
  versionOverrides: VersionOverrideWithParent[]
  genericVersionOverrides: VersionOverride[]
  convergeVersions: Map<string, string>
} {
  const [convergeOverrides, explicitOverrides] = partition(({ converge }) => converge === true, overrides)
  const [versionOverrides, genericVersionOverrides] = partition(({ parentPkg }) => parentPkg != null,
    explicitOverrides.map((override) => ({
      ...override,
      localTarget: createLocalTarget(override, rootDir),
    }))
  ) as [VersionOverrideWithParent[], VersionOverride[]]
  return {
    versionOverrides,
    genericVersionOverrides,
    convergeVersions: new Map(convergeOverrides.map((override) => [override.targetPkg.name, override.newBareSpecifier])),
  }
}

interface LocalTarget {
  /** Empty for a value written as a bare path, which carries none. */
  protocol: LocalProtocol | ''
  absolutePath: string
  specifiedViaRelativePath: boolean
}

type LocalProtocol = 'link:' | 'file:'

const isHomeRelative = /^~[/\\]/

function createLocalTarget (override: VersionOverrideWithoutRawSelector, rootDir: string): LocalTarget | undefined {
  let protocol: LocalProtocol | '' | undefined
  if (override.newBareSpecifier.startsWith('file:')) {
    protocol = 'file:'
  } else if (override.newBareSpecifier.startsWith('link:')) {
    protocol = 'link:'
  } else if (barePathIsUnambiguous(override.newBareSpecifier)) {
    protocol = ''
  } else {
    return undefined
  }
  const pkgPath = override.newBareSpecifier.substring(protocol.length)
  // A `~` path is expanded against the home directory by the resolver and
  // recorded verbatim, so it names the same place from everywhere. The
  // resolver forward-slashes a specifier before it reads that prefix, so
  // `~\` is the same path to it as `~/`.
  const specifiedViaRelativePath = !path.isAbsolute(pkgPath) && !isHomeRelative.test(pkgPath)
  const absolutePath = specifiedViaRelativePath ? path.join(rootDir, pkgPath) : pkgPath
  return { absolutePath, specifiedViaRelativePath, protocol }
}

interface VersionOverride extends VersionOverrideBase {
  localTarget?: LocalTarget
}

interface VersionOverrideWithParent extends VersionOverride {
  parentPkg: PackageSelector
}

function overrideDepsOfPkg (
  { manifest, dir }: { manifest: PackageManifest, dir: string | undefined },
  versionOverrides: VersionOverrideWithParent[],
  genericVersionOverrides: VersionOverride[],
  convergeOpts: ConvergeOptions
): void {
  const { dependencies, optionalDependencies, devDependencies, peerDependencies, peerDependenciesMeta } = manifest
  const _overrideDeps = overrideDeps.bind(null, { versionOverrides, genericVersionOverrides, dir, convergeOpts, peerDependenciesMeta })
  for (const deps of [dependencies, optionalDependencies, devDependencies]) {
    if (deps) {
      _overrideDeps(deps, undefined)
    }
  }
  if (peerDependencies) {
    if (!manifest.dependencies) manifest.dependencies = {}
    _overrideDeps(manifest.dependencies, peerDependencies)
  }
}

interface ConvergeOptions {
  convergeVersions: Map<string, string>
  convergeDeclaredRanges?: Map<string, Set<string>>
}

function overrideDeps (
  { versionOverrides, genericVersionOverrides, dir, convergeOpts, peerDependenciesMeta }: {
    versionOverrides: VersionOverrideWithParent[]
    genericVersionOverrides: VersionOverride[]
    dir: string | undefined
    convergeOpts: ConvergeOptions
    peerDependenciesMeta: PackageManifest['peerDependenciesMeta']
  },
  deps: Dependencies,
  peerDeps: Dependencies | undefined
): void {
  for (const [name, bareSpecifier] of Object.entries(peerDeps ?? deps)) {
    const versionOverride = pickVersionOverride({ versionOverrides, genericVersionOverrides }, name, bareSpecifier)
    if (!versionOverride) {
      convergeDep(convergeOpts, { deps, peerDeps }, name, bareSpecifier)
      continue
    }

    if (versionOverride.newBareSpecifier === '-') {
      if (peerDeps) {
        delete peerDeps[versionOverride.targetPkg.name]
        delete peerDependenciesMeta?.[versionOverride.targetPkg.name]
      } else {
        delete deps[versionOverride.targetPkg.name]
      }
      continue
    }

    const newBareSpecifier = resolveOverriddenBareSpecifier(versionOverride, dir)
    if (peerDeps == null || !isValidPeerRange(newBareSpecifier)) {
      deps[versionOverride.targetPkg.name] = newBareSpecifier
    } else if (isValidPeerRange(newBareSpecifier)) {
      peerDeps[versionOverride.targetPkg.name] = newBareSpecifier
    }
  }
}

function convergeDep (
  convergeOpts: ConvergeOptions,
  { deps, peerDeps }: { deps: Dependencies, peerDeps: Dependencies | undefined },
  name: string,
  bareSpecifier: string
): void {
  recordConvergeDeclaredRange(convergeOpts, name, bareSpecifier)
  const convergeVersion = convergeBareSpecifier(convergeOpts.convergeVersions, name, bareSpecifier)
  if (convergeVersion == null) return
  if (peerDeps == null) {
    deps[name] = convergeVersion
  } else {
    peerDeps[name] = convergeVersion
  }
}

function recordConvergeDeclaredRange (
  { convergeVersions, convergeDeclaredRanges }: ConvergeOptions,
  name: string,
  bareSpecifier: string
): void {
  if (convergeDeclaredRanges == null) return
  if (!convergeVersions.has(name) || semver.validRange(bareSpecifier, true) == null) return
  let ranges = convergeDeclaredRanges.get(name)
  if (ranges == null) {
    ranges = new Set()
    convergeDeclaredRanges.set(name, ranges)
  }
  ranges.add(bareSpecifier)
}

/**
 * A convergence override (`"pkg@": "<version>"`) rewrites a dependency edge
 * only when its version satisfies the edge's declared range, so incompatible
 * consumers keep their own resolution. Only plain semver ranges participate:
 * `workspace:`, `catalog:`, `npm:`, git/URL, and dist-tag specifiers have no
 * defined "satisfies" relation and are left untouched.
 */
function convergeBareSpecifier (
  convergeVersions: Map<string, string>,
  name: string,
  bareSpecifier: string
): string | undefined {
  const convergeVersion = convergeVersions.get(name)
  if (convergeVersion == null || semver.validRange(bareSpecifier, true) == null) return undefined
  return semver.satisfies(convergeVersion, bareSpecifier, true) ? convergeVersion : undefined
}

function pickVersionOverride (
  { versionOverrides, genericVersionOverrides }: {
    versionOverrides: VersionOverrideWithParent[]
    genericVersionOverrides: VersionOverride[]
  },
  name: string,
  bareSpecifier: string
): VersionOverride | undefined {
  const matches = (override: VersionOverride): boolean =>
    override.targetPkg.name === name && isIntersectingRange(override.targetPkg.bareSpecifier, bareSpecifier)
  return pickMostSpecificVersionOverride(versionOverrides.filter(matches)) ??
    pickMostSpecificVersionOverride(genericVersionOverrides.filter(matches))
}

function resolveOverriddenBareSpecifier (versionOverride: VersionOverride, dir: string | undefined): string {
  return versionOverride.localTarget
    ? `${versionOverride.localTarget.protocol}${resolveLocalOverride(versionOverride.localTarget, dir)}`
    : versionOverride.newBareSpecifier
}

function resolveLocalOverride ({ specifiedViaRelativePath, absolutePath, protocol }: LocalTarget, pkgDir?: string): string {
  const relative = specifiedViaRelativePath && pkgDir
    ? normalizePath(path.relative(pkgDir, absolutePath))
    : absolutePath
  // A target naming the consuming package's own directory diffs to the empty
  // string, which reads as a missing path rather than as "here".
  const resolved = relative === '' ? '.' : relative
  return protocol === '' ? renderBarePath(resolved) : resolved
}

/**
 * Render a path that carries no protocol, keeping it unambiguously local.
 *
 * Re-anchoring can drop a leading `./` — `./libs/x` measured from the
 * workspace root renders as `libs/x` — and a bare `<segment>/<segment>` reads
 * as a hosted-git shorthand instead, so the prefix goes back on.
 *
 * A drive-prefixed path cannot be made unambiguous by a prefix, since
 * `./C:/x` names something else entirely. A tarball takes `file:` instead,
 * which is what the local resolver resolves it under either way, so naming
 * the protocol cannot change how it materializes.
 *
 * A directory stays bare. Its protocol is not ours to choose: the resolver
 * reads a protocol-less directory as `link:` only while the dependency is not
 * injected, and as `file:` when it is. An explicit `link:` would outrank that
 * and silently reference an injected package in place instead of copying it,
 * so the drive-prefixed spelling keeps its ambiguity rather than trade it for
 * a wrong materialization.
 */
function renderBarePath (path: string): string {
  // Unlike a `file:` / `link:` value, which is emitted as written, a bare path
  // is spelled by pnpm rather than by the user, so it is spelled the way pnpm
  // v12 spells it: every separator forward, and nothing else touched.
  // `normalize-path` cannot stand in, because it also collapses repeated
  // separators, and a UNC share's leading `\\` is what makes it a share.
  const spec = path.replace(/\\/g, '/')
  if (isDriveLetterPrefix(spec)) {
    return isTarballFilename(spec) ? `file:${spec}` : spec
  }
  return isFilespec(spec) ? spec : `./${spec}`
}

function pickMostSpecificVersionOverride (versionOverrides: VersionOverride[]): VersionOverride | undefined {
  return versionOverrides.sort((a, b) => isIntersectingRange(b.targetPkg.bareSpecifier ?? '', a.targetPkg.bareSpecifier ?? '') ? -1 : 1)[0]
}
