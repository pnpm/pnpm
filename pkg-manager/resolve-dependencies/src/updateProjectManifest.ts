import {
  createVersionSpec,
  getPrefix,
  type PackageSpecObject,
  type PinnedVersion,
  updateProjectManifestObject,
} from '@pnpm/manifest-utils'
import versionSelectorType from 'version-selector-type'
import semver from 'semver'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import { type TarballResolution } from '@pnpm/resolver-base'
import { type ResolvedDirectDependency } from './resolveDependencyTree'
import { type ImporterToResolve } from '.'

export async function updateProjectManifest (
  importer: ImporterToResolve,
  opts: {
    directDependencies: ResolvedDirectDependency[]
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: boolean | 'rolling'
  }
) {
  if (!importer.manifest) {
    throw new Error('Cannot save because no package.json found')
  }
  const specsToUpsert = opts.directDependencies
    .filter((rdd, index) => importer.wantedDependencies[index]?.updateSpec)
    .map((rdd, index) => {
      const wantedDep = importer.wantedDependencies[index]!
      return resolvedDirectDepToSpecObject({
        ...rdd,
        isNew:
        wantedDep.isNew,
        specRaw: wantedDep.raw,
        preserveNonSemverVersionSpec: wantedDep.preserveNonSemverVersionSpec,
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
    isNew,
    name,
    normalizedPref,
    resolution,
    specRaw,
    version,
    preserveNonSemverVersionSpec,
  }: ResolvedDirectDependency & { isNew?: boolean, specRaw: string, preserveNonSemverVersionSpec?: boolean },
  importer: ImporterToResolve,
  opts: {
    nodeExecPath?: string
    pinnedVersion: PinnedVersion
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: boolean | 'rolling'
  }
): PackageSpecObject {
  let pref!: string
  if (normalizedPref) {
    pref = normalizedPref
  } else {
    const shouldUseWorkspaceProtocol = resolution.type === 'directory' &&
      (
        Boolean(opts.saveWorkspaceProtocol) ||
        (opts.preserveWorkspaceProtocol && specRaw.includes('@workspace:'))
      ) &&
      opts.pinnedVersion !== 'none'

    if (isNew === true) {
      pref = getPrefPreferSpecifiedSpec({
        alias,
        name,
        pinnedVersion: opts.pinnedVersion,
        specRaw,
        version,
        rolling: shouldUseWorkspaceProtocol && opts.saveWorkspaceProtocol === 'rolling',
      })
    } else {
      pref = getPrefPreferSpecifiedExoticSpec({
        alias,
        name,
        pinnedVersion: opts.pinnedVersion,
        specRaw,
        version,
        rolling: shouldUseWorkspaceProtocol && opts.saveWorkspaceProtocol === 'rolling',
        preserveNonSemverVersionSpec,
      })
    }
    if (
      shouldUseWorkspaceProtocol &&
      !pref.startsWith('workspace:')
    ) {
      pref = `workspace:${pref}`
    }
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
    specRaw: string
    pinnedVersion?: PinnedVersion
    rolling: boolean
  }
) {
  const prefix = getPrefix(opts.alias, opts.name)
  if (opts.specRaw?.startsWith(`${opts.alias}@${prefix}`)) {
    const range = opts.specRaw.slice(`${opts.alias}@${prefix}`.length)
    if (range) {
      const selector = versionSelectorType(range)
      if ((selector != null) && (selector.type === 'version' || selector.type === 'range')) {
        return opts.specRaw.slice(opts.alias.length + 1)
      }
    }
  }
  // A prerelease version is always added as an exact version
  if (semver.parse(opts.version)?.prerelease.length) {
    return `${prefix}${opts.version}`
  }
  return `${prefix}${createVersionSpec(opts.version, { pinnedVersion: opts.pinnedVersion, rolling: opts.rolling })}`
}

function getPrefPreferSpecifiedExoticSpec (
  opts: {
    alias: string
    name: string
    version: string
    specRaw: string
    pinnedVersion: PinnedVersion
    rolling: boolean
    preserveNonSemverVersionSpec?: boolean
  }
) {
  const prefix = getPrefix(opts.alias, opts.name)
  if (opts.specRaw?.startsWith(`${opts.alias}@${prefix}`)) {
    let specWithoutName = opts.specRaw.slice(`${opts.alias}@${prefix}`.length)
    if (specWithoutName.startsWith('workspace:')) {
      specWithoutName = specWithoutName.slice(10)
      if (specWithoutName === '*' || specWithoutName === '^' || specWithoutName === '~') {
        return specWithoutName
      }
    }
    const selector = versionSelectorType(specWithoutName)
    if (
      ((selector == null) || (selector.type !== 'version' && selector.type !== 'range')) &&
      opts.preserveNonSemverVersionSpec
    ) {
      return opts.specRaw.slice(opts.alias.length + 1)
    }
  }
  // A prerelease version is always added as an exact version
  if (semver.parse(opts.version)?.prerelease.length) {
    return `${prefix}${opts.version}`
  }

  return `${prefix}${createVersionSpec(opts.version, { pinnedVersion: opts.pinnedVersion, rolling: opts.rolling })}`
}
