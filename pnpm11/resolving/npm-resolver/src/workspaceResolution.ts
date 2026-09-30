import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import type {
  PkgResolutionId,
  WantedDependency,
  WorkspacePackage,
  WorkspacePackages,
  WorkspacePackagesByVersion,
} from '@pnpm/resolving.resolver-base'
import type { RangeSpecStyle } from '@pnpm/types'
import { resolveWorkspaceRange } from '@pnpm/workspace.range-resolver'
import normalize from 'normalize-path'
import { clone } from 'ramda'
import semver from 'semver'

import { calcSpecifierForWorkspaceDep } from './calcSpecifier.js'
import { parseBareSpecifier, type RegistryPackageSpec } from './parseBareSpecifier.js'
import type { WorkspaceResolveResult } from './resolverTypes.js'
import { workspacePrefToNpm } from './workspacePrefToNpm.js'

export interface LocalPackageResolutionOptions {
  wantedDependency: WantedDependency
  hardLinkLocalPackages?: boolean
  projectDir: string
  lockfileDir?: string
  update?: boolean
  updateRequested?: boolean
  saveWorkspaceProtocol?: boolean | 'rolling'
  calcSpecifier?: boolean
  rangeSpecStyle?: RangeSpecStyle
}

export function tryResolveFromWorkspace (
  wantedDependency: WantedDependency,
  opts: {
    defaultTag: string
    lockfileDir?: string
    projectDir?: string
    registry: string
    workspacePackages?: WorkspacePackages
    injectWorkspacePackages?: boolean
    update?: boolean
    updateRequested?: boolean
    saveWorkspaceProtocol?: boolean | 'rolling'
    calcSpecifier?: boolean
    rangeSpecStyle?: RangeSpecStyle
  }
): WorkspaceResolveResult | null {
  if (!wantedDependency.bareSpecifier?.startsWith('workspace:')) {
    return null
  }
  const bareSpecifier = workspacePrefToNpm(wantedDependency.bareSpecifier)

  const spec = parseBareSpecifier(bareSpecifier, wantedDependency.alias, opts.defaultTag, opts.registry)
  if (spec == null) throw new Error(`Invalid workspace: spec (${wantedDependency.bareSpecifier})`)
  if (opts.workspacePackages == null) {
    throw new Error('Cannot resolve package from workspace because opts.workspacePackages is not defined')
  }
  if (!opts.projectDir) {
    throw new Error('Cannot resolve package from workspace because opts.projectDir is not defined')
  }
  return tryResolveFromWorkspacePackages(opts.workspacePackages, spec, {
    wantedDependency,
    projectDir: opts.projectDir,
    hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
    lockfileDir: opts.lockfileDir,
    update: opts.update,
    updateRequested: opts.updateRequested,
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    calcSpecifier: opts.calcSpecifier,
    rangeSpecStyle: opts.rangeSpecStyle,
  })
}

export function tryResolveFromWorkspacePackages (
  workspacePackages: WorkspacePackages,
  spec: RegistryPackageSpec,
  opts: LocalPackageResolutionOptions
): WorkspaceResolveResult {
  const workspacePkgsMatchingName = workspacePackages.get(spec.name)
  if (!workspacePkgsMatchingName) {
    throw new PnpmError(
      'WORKSPACE_PKG_NOT_FOUND',
      `In ${path.relative(process.cwd(), opts.projectDir)}: "${spec.name}@${opts.wantedDependency.bareSpecifier ?? ''}" is in the dependencies but no package named "${spec.name}" is present in the workspace`,
      {
        hint: 'Packages found in the workspace: ' + Array.from(workspacePackages.keys()).join(', '),
      }
    )
  }
  const localVersion = pickMatchingLocalVersionOrNull(
    workspacePkgsMatchingName,
    opts.update ? { name: spec.name, fetchSpec: '*', type: 'range' } : spec
  )
  if (!localVersion) {
    const availableVersions = Array.from(workspacePkgsMatchingName.keys()).sort(rcompareVersions)
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `In ${path.relative(process.cwd(), opts.projectDir)}: No matching version found for ${opts.wantedDependency.alias ?? ''}@${opts.wantedDependency.bareSpecifier ?? ''} inside the workspace` +
      (availableVersions.length ? `. Available versions: ${availableVersions.join(', ')}` : ''),
      availableVersions.length
        ? {
          hint: `Available workspace versions for "${spec.name}": ${availableVersions.join(', ')}`,
        }
        : undefined
    )
  }
  return resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, spec, opts)
}

export function pickMatchingLocalVersionOrNull (
  versions: WorkspacePackagesByVersion,
  spec: RegistryPackageSpec
): string | null {
  switch (spec.type) {
    case 'tag':
      return resolveWorkspaceRange('*', Array.from(versions.keys()))
    case 'version':
      if (versions.has(spec.fetchSpec)) return spec.fetchSpec
      return resolveWorkspaceRange(spec.fetchSpec, Array.from(versions.keys()))
    case 'range':
      return resolveWorkspaceRange(spec.fetchSpec, Array.from(versions.keys()))
    default:
      return null
  }
}

function rcompareVersions (leftVersion: string, rightVersion: string): number {
  const leftIsSemver = semver.valid(leftVersion) != null
  const rightIsSemver = semver.valid(rightVersion) != null
  if (leftIsSemver !== rightIsSemver) return leftIsSemver ? -1 : 1
  const bySemver = leftIsSemver ? semver.rcompare(leftVersion, rightVersion) : 0
  return bySemver || (rightVersion < leftVersion ? -1 : rightVersion > leftVersion ? 1 : 0)
}

export function resolveFromLocalPackage (
  localPackage: WorkspacePackage,
  spec: RegistryPackageSpec,
  opts: LocalPackageResolutionOptions
): WorkspaceResolveResult {
  let id!: PkgResolutionId
  let directory!: string
  const localPackageDir = resolveLocalPackageDir(localPackage)
  if (opts.hardLinkLocalPackages) {
    directory = normalize(path.relative(opts.lockfileDir!, localPackageDir))
    id = `file:${directory}` as PkgResolutionId
  } else {
    directory = localPackageDir
    id = `link:${normalize(path.relative(opts.projectDir, localPackageDir))}` as PkgResolutionId
  }
  let normalizedBareSpecifier: string | undefined
  if (opts.calcSpecifier) {
    normalizedBareSpecifier = spec.normalizedBareSpecifier ?? calcSpecifierForWorkspaceDep({
      wantedDependency: opts.wantedDependency,
      spec,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      version: localPackage.manifest.version,
      defaultRangeSpecStyle: opts.rangeSpecStyle,
      isUpdate: opts.updateRequested,
    })
  }
  return {
    id,
    manifest: clone(localPackage.manifest),
    resolution: {
      directory,
      type: 'directory',
    },
    resolvedVia: 'workspace',
    normalizedBareSpecifier,
  }
}

function resolveLocalPackageDir (localPackage: WorkspacePackage): string {
  if (
    localPackage.manifest.publishConfig?.directory == null ||
    localPackage.manifest.publishConfig?.linkDirectory === false
  ) return localPackage.rootDir
  return path.join(localPackage.rootDir, localPackage.manifest.publishConfig.directory)
}
