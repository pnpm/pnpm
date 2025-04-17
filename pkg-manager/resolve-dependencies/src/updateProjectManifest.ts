import {
  createVersionSpec,
  type PackageSpecObject,
  type PinnedVersion,
  updateProjectManifestObject,
} from '@pnpm/manifest-utils'
import semver from 'semver'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import { type TarballResolution } from '@pnpm/resolver-base'
import { type ProjectManifest } from '@pnpm/types'
import { type ResolvedDirectDependency } from './resolveDependencyTree'
import { type ImporterToResolve } from '.'

export async function updateProjectManifest (
  importer: ImporterToResolve,
  opts: {
    directDependencies: ResolvedDirectDependency[]
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: boolean | 'rolling'
  }
): Promise<Array<ProjectManifest | undefined>> {
  if (!importer.manifest) {
    throw new Error('Cannot save because no package.json found')
  }
  const specsToUpsert = opts.directDependencies
    .filter((rdd, index) => importer.wantedDependencies[index]?.updateSpec)
    .map((rdd, index) => {
      const wantedDep = importer.wantedDependencies[index]!
      return resolvedDirectDepToSpecObject({
        ...rdd,
        currentPref: wantedDep.pref,
        // For git-protocol dependencies that are already installed locally, there is no normalizedPref unless do force resolve,
        // so we use pref in wantedDependency here.
        normalizedPref: rdd.normalizedPref ?? (isGitHostedPkgUrl((rdd.resolution as TarballResolution).tarball ?? '') ? wantedDep.pref : undefined),
      }, importer, {
        nodeExecPath: wantedDep.nodeExecPath,
        pinnedVersion: wantedDep.pinnedVersion ?? importer.pinnedVersion ?? 'major',
        preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
        saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      })
    })
  for (const pkgToInstall of importer.wantedDependencies) {
    if (pkgToInstall.updateSpec && pkgToInstall.alias && !specsToUpsert.some(({ alias }) => alias === pkgToInstall.alias)) {
      specsToUpsert.push({
        alias: pkgToInstall.alias,
        nodeExecPath: pkgToInstall.nodeExecPath,
        peer: importer.peer,
        saveType: importer.targetDependenciesField,
      })
    }
  }
  const hookedManifest = await updateProjectManifestObject(
    importer.rootDir,
    importer.manifest,
    specsToUpsert
  )
  const originalManifest = (importer.originalManifest != null)
    ? await updateProjectManifestObject(
      importer.rootDir,
      importer.originalManifest,
      specsToUpsert
    )
    : undefined
  return [hookedManifest, originalManifest]
}

function resolvedDirectDepToSpecObject (
  {
    alias,
    catalogLookup,
    name,
    normalizedPref,
    currentPref,
    version,
    specifierTemplate,
  }: Omit<ResolvedDirectDependency, 'resolution'> & { currentPref: string },
  importer: ImporterToResolve,
  opts: {
    nodeExecPath?: string
    pinnedVersion: PinnedVersion
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: boolean | 'rolling'
  }
): PackageSpecObject {
  let pref!: string
  if (catalogLookup) {
    pref = catalogLookup.userSpecifiedPref
  } else if (normalizedPref) {
    pref = normalizedPref
  } else {
    pref = getPrefPreferSpecifiedSpec({
      alias,
      name,
      pinnedVersion: opts.pinnedVersion,
      currentPref,
      version,
      rolling: opts.saveWorkspaceProtocol === 'rolling',
      specifierTemplate: specifierTemplate!,
    })
  }
  return {
    alias,
    nodeExecPath: opts.nodeExecPath,
    peer: importer['peer'],
    pref,
    saveType: importer['targetDependenciesField'],
  }
}

function getPrefPreferSpecifiedSpec (
  opts: {
    alias: string
    name: string
    version: string
    currentPref: string
    pinnedVersion?: PinnedVersion
    rolling: boolean
    specifierTemplate: string
  }
): string {
  if (!opts.specifierTemplate?.endsWith('<range>')) {
    return opts.specifierTemplate ?? opts.currentPref
  }
  const prefix = opts.specifierTemplate.substring(0, opts.specifierTemplate.length - '<range>'.length)
  // A prerelease version is always added as an exact version
  if (semver.parse(opts.version)?.prerelease.length) {
    return `${prefix}${opts.version}`
  }
  return `${prefix}${createVersionSpec(opts.version, { pinnedVersion: opts.pinnedVersion, rolling: prefix.startsWith('workspace:') && opts.rolling })}`
}
