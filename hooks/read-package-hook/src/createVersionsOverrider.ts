import path from 'path'
import semver from 'semver'
import partition from 'ramda/src/partition'
import { type Dependencies, type PackageManifest, type ReadPackageHook } from '@pnpm/types'
import { type PackageSelector, type VersionOverride as VersionOverrideBase } from '@pnpm/parse-overrides'
import { isValidPeerRange } from '@pnpm/semver.peer-range'
import normalizePath from 'normalize-path'
import { isIntersectingRange } from './isIntersectingRange'

export type VersionOverrideWithoutRawSelector = Omit<VersionOverrideBase, 'selector'>

export function createVersionsOverrider (
  overrides: VersionOverrideWithoutRawSelector[],
  rootDir: string
): ReadPackageHook {
  const [versionOverrides, genericVersionOverrides] = partition(({ parentPkg }) => parentPkg != null,
    overrides.map((override) => ({
      ...override,
      localTarget: createLocalTarget(override, rootDir),
    }))
  ) as [VersionOverrideWithParent[], VersionOverride[]]
  return ((manifest: PackageManifest, dir?: string) => {
    const versionOverridesWithParent = versionOverrides.filter(({ parentPkg }) => {
      return (
        parentPkg.name === manifest.name &&
        (!parentPkg.pref || semver.satisfies(manifest.version, parentPkg.pref))
      )
    })
    overrideDepsOfPkg({ manifest, dir }, versionOverridesWithParent, genericVersionOverrides)

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
  if (override.newPref.startsWith('file:')) {
    protocol = 'file:'
  } else if (override.newPref.startsWith('link:')) {
    protocol = 'link:'
  } else {
    return undefined
  }
  const pkgPath = override.newPref.substring(protocol.length)
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
  genericVersionOverrides: VersionOverride[]
): void {
  const { dependencies, optionalDependencies, devDependencies, peerDependencies } = manifest
  const _overrideDeps = overrideDeps.bind(null, { versionOverrides, genericVersionOverrides, dir })
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

function overrideDeps (
  { versionOverrides, genericVersionOverrides, dir }: {
    versionOverrides: VersionOverrideWithParent[]
    genericVersionOverrides: VersionOverride[]
    dir: string | undefined
  },
  deps: Dependencies,
  peerDeps: Dependencies | undefined
): void {
  for (const [name, pref] of Object.entries(peerDeps ?? deps)) {
    const versionOverride =
    pickMostSpecificVersionOverride(
      versionOverrides.filter(
        ({ targetPkg }) =>
          targetPkg.name === name && isIntersectingRange(targetPkg.pref, pref)
      )
    ) ??
    pickMostSpecificVersionOverride(
      genericVersionOverrides.filter(
        ({ targetPkg }) =>
          targetPkg.name === name && isIntersectingRange(targetPkg.pref, pref)
      )
    )
    if (!versionOverride) continue

    if (versionOverride.newPref === '-') {
      if (peerDeps) {
        delete peerDeps[versionOverride.targetPkg.name]
      } else {
        delete deps[versionOverride.targetPkg.name]
      }
      continue
    }

    const newPref = versionOverride.localTarget
      ? `${versionOverride.localTarget.protocol}${resolveLocalOverride(versionOverride.localTarget, dir)}`
      : versionOverride.newPref
    if (peerDeps == null || !isValidPeerRange(newPref)) {
      deps[versionOverride.targetPkg.name] = newPref
    } else if (isValidPeerRange(newPref)) {
      peerDeps[versionOverride.targetPkg.name] = newPref
    }
  }
}

function resolveLocalOverride ({ specifiedViaRelativePath, absolutePath }: LocalTarget, pkgDir?: string): string {
  return specifiedViaRelativePath && pkgDir
    ? normalizePath(path.relative(pkgDir, absolutePath))
    : absolutePath
}

function pickMostSpecificVersionOverride (versionOverrides: VersionOverride[]): VersionOverride | undefined {
  return versionOverrides.sort((a, b) => isIntersectingRange(b.targetPkg.pref ?? '', a.targetPkg.pref ?? '') ? -1 : 1)[0]
}
