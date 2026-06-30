import path from 'node:path'

import type { VersionOverride as VersionOverrideBase } from '@pnpm/config.parse-overrides'
import { isValidPeerRange } from '@pnpm/deps.peer-range'
import type { Dependencies, PackageManifest, ReadPackageHook } from '@pnpm/types'
import normalizePath from 'normalize-path'
import { partition } from 'ramda'
import semver from 'semver'

import { isIntersectingRange } from './isIntersectingRange.js'

/**
 * @deprecated Kept for backward compatibility with external consumers. New
 * code should use `VersionOverride` from `@pnpm/config.parse-overrides`
 * directly — the raw `selector` field is needed for the post-resolution
 * unused-override check and there is no longer a use case for the stripped
 * shape inside this repo.
 */
export type VersionOverrideWithoutRawSelector = Omit<VersionOverrideBase, 'selector'>

type VersionOverrideInput = VersionOverrideBase | VersionOverrideWithoutRawSelector

export function createVersionsOverrider (
  overrides: VersionOverrideInput[],
  rootDir: string,
  onApplied?: (override: VersionOverrideBase) => void
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
    overrideDepsOfPkg(
      { manifest, dir, onApplied },
      versionOverridesWithParent,
      genericVersionOverrides
    )

    return manifest
  }) as ReadPackageHook
}

interface LocalTarget {
  protocol: LocalProtocol
  absolutePath: string
  specifiedViaRelativePath: boolean
}

type LocalProtocol = 'link:' | 'file:'

function createLocalTarget (override: VersionOverrideInput, rootDir: string): LocalTarget | undefined {
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

type VersionOverride = VersionOverrideInput & {
  localTarget?: LocalTarget
}

type VersionOverrideWithParent = VersionOverride & {
  parentPkg: NonNullable<VersionOverrideBase['parentPkg']>
}

function overrideDepsOfPkg (
  { manifest, dir, onApplied }: {
    manifest: PackageManifest
    dir: string | undefined
    onApplied?: (override: VersionOverrideBase) => void
  },
  versionOverrides: VersionOverrideWithParent[],
  genericVersionOverrides: VersionOverride[]
): void {
  const { dependencies, optionalDependencies, devDependencies, peerDependencies } = manifest
  const _overrideDeps = overrideDeps.bind(null, { versionOverrides, genericVersionOverrides, dir, onApplied })
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
  { versionOverrides, genericVersionOverrides, dir, onApplied }: {
    versionOverrides: VersionOverrideWithParent[]
    genericVersionOverrides: VersionOverride[]
    dir: string | undefined
    onApplied?: (override: VersionOverrideBase) => void
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

    if (hasRawSelector(versionOverride)) onApplied?.(versionOverride)

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

function hasRawSelector (override: VersionOverride): override is VersionOverrideBase & { localTarget?: LocalTarget } {
  return 'selector' in override
}
