import path from 'node:path'

import type { PackageSelector, VersionOverride as VersionOverrideBase } from '@pnpm/config.parse-overrides'
import { isValidPeerRange } from '@pnpm/deps.peer-range'
import type { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import normalizePath from 'normalize-path'
import { partition } from 'ramda'
import semver from 'semver'

import { isIntersectingRange } from './isIntersectingRange.js'

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
        (!parentPkg.bareSpecifier || semver.satisfies(manifest.version, parentPkg.bareSpecifier))
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

const TARBALL_SPECIFIER_PATTERN = /\.(?:tgz|tar\.gz|tar)$/i
const NAMED_PROTOCOL_PATTERN = /^[a-z][a-z0-9+.-]*:/i
const WINDOWS_ABSOLUTE_PATH_PATTERN = /^[a-z]:[/\\]/i

function createLocalTarget (override: VersionOverrideWithoutRawSelector, rootDir: string): LocalTarget | undefined {
  const localSpecifier = parseLocalOverrideSpecifier(override.newBareSpecifier)
  if (!localSpecifier) {
    return undefined
  }
  const pkgPath = localSpecifier.path
  const specifiedViaRelativePath = !path.isAbsolute(pkgPath)
  const absolutePath = specifiedViaRelativePath ? path.join(rootDir, pkgPath) : pkgPath
  return { absolutePath, specifiedViaRelativePath, protocol: localSpecifier.protocol }
}

function parseLocalOverrideSpecifier (
  newBareSpecifier: string
): { protocol: LocalProtocol, path: string } | undefined {
  if (newBareSpecifier.startsWith('file:')) {
    return { protocol: 'file:', path: newBareSpecifier.substring('file:'.length) }
  }
  if (newBareSpecifier.startsWith('link:')) {
    return { protocol: 'link:', path: newBareSpecifier.substring('link:'.length) }
  }
  if (!isLocalTarballSpecifier(newBareSpecifier)) {
    return undefined
  }
  return { protocol: 'file:', path: newBareSpecifier }
}

function isLocalTarballSpecifier (specifier: string): boolean {
  if (!TARBALL_SPECIFIER_PATTERN.test(specifier)) return false
  return WINDOWS_ABSOLUTE_PATH_PATTERN.test(specifier) || !NAMED_PROTOCOL_PATTERN.test(specifier)
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
    if (!versionOverride) continue

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

function resolveLocalOverride ({ specifiedViaRelativePath, absolutePath }: LocalTarget, pkgDir?: string): string {
  return specifiedViaRelativePath && pkgDir
    ? normalizePath(path.relative(pkgDir, absolutePath))
    : absolutePath
}

function pickMostSpecificVersionOverride (versionOverrides: VersionOverride[]): VersionOverride | undefined {
  return versionOverrides.sort((a, b) => isIntersectingRange(b.targetPkg.bareSpecifier ?? '', a.targetPkg.bareSpecifier ?? '') ? -1 : 1)[0]
}
