import path from 'node:path'

import type { PackageSelector, VersionOverride as VersionOverrideBase } from '@pnpm/config.parse-overrides'
import { isValidPeerRange } from '@pnpm/deps.peer-range'
import type { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
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

export function createVersionsOverrider (
  overrides: VersionOverrideWithoutRawSelector[],
  rootDir: string,
  opts?: CreateVersionsOverriderOptions
): ReadPackageHook {
  const [convergeOverrides, explicitOverrides] = partition(({ converge }) => converge === true, overrides)
  const [versionOverrides, genericVersionOverrides] = partition(({ parentPkg }) => parentPkg != null,
    explicitOverrides.map((override) => ({
      ...override,
      localTarget: createLocalTarget(override, rootDir),
    }))
  ) as [VersionOverrideWithParent[], VersionOverride[]]
  const convergeVersions = new Map(convergeOverrides.map((override) => [override.targetPkg.name, override.newBareSpecifier]))
  return ((manifest: PackageManifest, dir?: string) => {
    const versionOverridesWithParent = versionOverrides.filter(({ parentPkg }) => {
      return (
        parentPkg.name === manifest.name &&
        (!parentPkg.bareSpecifier || semver.satisfies(manifest.version, parentPkg.bareSpecifier))
      )
    })
    overrideDepsOfPkg({ manifest, dir }, versionOverridesWithParent, genericVersionOverrides, {
      convergeVersions,
      convergeDeclaredRanges: opts?.convergeDeclaredRanges,
    })

    return manifest
  }) as ReadPackageHook
}

interface LocalTarget {
  protocol: LocalProtocol
  absolutePath: string
  specifiedViaRelativePath: boolean
}

type LocalProtocol = 'link:' | 'file:'

function createLocalTarget (override: VersionOverrideWithoutRawSelector, rootDir: string): LocalTarget | undefined {
  let protocol: LocalProtocol | undefined
  if (override.newBareSpecifier.startsWith('file:')) {
    protocol = 'file:'
  } else if (override.newBareSpecifier.startsWith('link:')) {
    protocol = 'link:'
  } else {
    return undefined
  }
  const pkgPath = override.newBareSpecifier.substring(protocol.length)
  const specifiedViaRelativePath = !path.isAbsolute(pkgPath)
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
  const { dependencies, optionalDependencies, devDependencies, peerDependencies } = manifest
  const _overrideDeps = overrideDeps.bind(null, { versionOverrides, genericVersionOverrides, dir, convergeOpts })
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
  { versionOverrides, genericVersionOverrides, dir, convergeOpts }: {
    versionOverrides: VersionOverrideWithParent[]
    genericVersionOverrides: VersionOverride[]
    dir: string | undefined
    convergeOpts: ConvergeOptions
  },
  deps: Dependencies,
  peerDeps: Dependencies | undefined
): void {
  for (const [name, bareSpecifier] of Object.entries(peerDeps ?? deps)) {
    const versionOverride =
      pickMostSpecificVersionOverride(
        versionOverrides.filter(
          ({ targetPkg }) =>
            targetPkg.name === name && isIntersectingRange(targetPkg.bareSpecifier, bareSpecifier)
        )
      ) ??
    pickMostSpecificVersionOverride(
      genericVersionOverrides.filter(
        ({ targetPkg }) =>
          targetPkg.name === name && isIntersectingRange(targetPkg.bareSpecifier, bareSpecifier)
      )
    )
    if (!versionOverride) {
      convergeDep(convergeOpts, { deps, peerDeps }, name, bareSpecifier)
      continue
    }

    if (versionOverride.newBareSpecifier === '-') {
      if (peerDeps) {
        delete peerDeps[versionOverride.targetPkg.name]
      } else {
        delete deps[versionOverride.targetPkg.name]
      }
      continue
    }

    const newBareSpecifier = versionOverride.localTarget
      ? `${versionOverride.localTarget.protocol}${resolveLocalOverride(versionOverride.localTarget, dir)}`
      : versionOverride.newBareSpecifier
    if (peerDeps == null || !isValidPeerRange(newBareSpecifier)) {
      deps[versionOverride.targetPkg.name] = newBareSpecifier
    } else if (isValidPeerRange(newBareSpecifier)) {
      peerDeps[versionOverride.targetPkg.name] = newBareSpecifier
    }
  }
}

/**
 * A convergence override (`"pkg@": "<version>"`) rewrites a dependency edge
 * only when its version satisfies the edge's declared range, so incompatible
 * consumers keep their own resolution. Only plain semver ranges participate:
 * `workspace:`, `catalog:`, `npm:`, git/URL, and dist-tag specifiers have no
 * defined "satisfies" relation and are left untouched.
 */
function convergeDep (
  { convergeVersions, convergeDeclaredRanges }: ConvergeOptions,
  { deps, peerDeps }: { deps: Dependencies, peerDeps: Dependencies | undefined },
  name: string,
  bareSpecifier: string
): void {
  const convergeVersion = convergeVersions.get(name)
  if (convergeVersion == null || semver.validRange(bareSpecifier, true) == null) return
  if (convergeDeclaredRanges != null) {
    let ranges = convergeDeclaredRanges.get(name)
    if (ranges == null) {
      ranges = new Set()
      convergeDeclaredRanges.set(name, ranges)
    }
    ranges.add(bareSpecifier)
  }
  if (!semver.satisfies(convergeVersion, bareSpecifier, true)) return
  if (peerDeps == null) {
    deps[name] = convergeVersion
  } else {
    peerDeps[name] = convergeVersion
  }
}

function resolveLocalOverride ({ specifiedViaRelativePath, absolutePath }: LocalTarget, pkgDir?: string): string {
  return specifiedViaRelativePath && pkgDir
    ? normalizePath(path.relative(pkgDir, absolutePath))
    : absolutePath
}

function pickMostSpecificVersionOverride (versionOverrides: VersionOverride[]): VersionOverride | undefined {
  return versionOverrides.sort((a, b) => isIntersectingRange(b.targetPkg.bareSpecifier ?? '', a.targetPkg.bareSpecifier ?? '') ? -1 : 1)[0]
}
