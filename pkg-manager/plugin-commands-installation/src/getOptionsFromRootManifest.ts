import { PnpmError } from '@pnpm/error'
import {
  type AllowedDeprecatedVersions,
  type PackageExtension,
  type PeerDependencyRules,
  type ProjectManifest,
} from '@pnpm/types'
import mapValues from 'ramda/src/map'
import path from 'path'
import fs from 'fs'

function checkOverrides (overrides: Record<string, string>) {
  Object.keys(overrides).forEach(key => {
    const value = overrides[key]
    if (value.startsWith('link:')) {
      const _path = path.isAbsolute(value) ? value : path.join(process.cwd(), value)
      if (!fs.existsSync(_path)) {
        throw new PnpmError(
          'CANNOT_RESOLVE_OVERRIDE',
          `Cannot resolve package ${key} in overrides. The address of the package link is incorrect.`
        )
      }
    }
  })
}

export function getOptionsFromRootManifest (manifest: ProjectManifest): {
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowNonAppliedPatches?: boolean
  overrides?: Record<string, string>
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  packageExtensions?: Record<string, PackageExtension>
  patchedDependencies?: Record<string, string>
  peerDependencyRules?: PeerDependencyRules
} {
  // We read Yarn's resolutions field for compatibility
  // but we really replace the version specs to any other version spec, not only to exact versions,
  // so we cannot call it resolutions
  const overrides = mapValues(
    createVersionReferencesReplacer(manifest),
    manifest.pnpm?.overrides ?? manifest.resolutions ?? {}
  )
  checkOverrides(overrides)
  const neverBuiltDependencies = manifest.pnpm?.neverBuiltDependencies
  const onlyBuiltDependencies = manifest.pnpm?.onlyBuiltDependencies
  const packageExtensions = manifest.pnpm?.packageExtensions
  const peerDependencyRules = manifest.pnpm?.peerDependencyRules
  const allowedDeprecatedVersions = manifest.pnpm?.allowedDeprecatedVersions
  const allowNonAppliedPatches = manifest.pnpm?.allowNonAppliedPatches
  const patchedDependencies = manifest.pnpm?.patchedDependencies
  const settings = {
    allowedDeprecatedVersions,
    allowNonAppliedPatches,
    overrides,
    neverBuiltDependencies,
    packageExtensions,
    peerDependencyRules,
    patchedDependencies,
  }
  if (onlyBuiltDependencies) {
    // @ts-expect-error
    settings['onlyBuiltDependencies'] = onlyBuiltDependencies
  }
  return settings
}

function createVersionReferencesReplacer (manifest: ProjectManifest) {
  const allDeps = {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
  return replaceVersionReferences.bind(null, allDeps)
}

function replaceVersionReferences (dep: Record<string, string>, spec: string) {
  if (!spec.startsWith('$')) return spec
  const dependencyName = spec.slice(1)
  const newSpec = dep[dependencyName]
  if (newSpec) return newSpec
  throw new PnpmError(
    'CANNOT_RESOLVE_OVERRIDE_VERSION',
    `Cannot resolve version ${spec} in overrides. The direct dependencies don't have dependency "${dependencyName}".`
  )
}
