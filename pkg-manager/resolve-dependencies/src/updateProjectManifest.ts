import * as jsr from '@pnpm/jsr-specs'
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
    catalogLookup,
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
  if (catalogLookup) {
    pref = catalogLookup.userSpecifiedPref
  } else if (normalizedPref) {
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
      pref = pref.replace(/^npm:/, '')
      pref = `workspace:${pref}`
    } else {
      pref = getJsrPref({
        alias,
        pinnedVersion: opts.pinnedVersion,
        prefix: '',
        specRaw,
        version,
      }) ?? getJsrPref({
        alias,
        pinnedVersion: opts.pinnedVersion,
        prefix: `${alias}@`,
        specRaw,
        version,
      }) ?? pref
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

function getJsrPref ({
  alias,
  pinnedVersion,
  prefix,
  specRaw,
  version,
}: {
  alias: string
  pinnedVersion?: PinnedVersion
  prefix: '' | `${string}@`
  specRaw: string
  version: string
}): string | undefined {
  if (!specRaw.startsWith(prefix)) return undefined
  const specWithoutPrefix = specRaw.slice(prefix.length)

  const spec = jsr.parseJsrSpec(specWithoutPrefix)
  if (spec == null) return undefined

  if (spec.spec == null || spec.spec === 'latest') {
    spec.spec = createVersionSpec(version, {
      pinnedVersion,
      rolling: false, // always false because it's definitely not a workspace protocol
    })
  }

  // syntax: [<name>@]jsr:@<real_scope>/<real_name>[@<spec>]
  if (spec.scope != null) {
    const jsrPackageName = jsr.createJsrPackageName(spec)
    return jsr.createJsrPref(
      jsrPackageName === alias
        ? { spec: spec.spec } // omit the alias from the pref
        : spec
    )
  }

  // syntax: jsr:<spec>
  if (prefix === '') {
    return jsr.createJsrPref(spec)
  }

  // syntax: <name>@jsr:<spec>
  const parsed: jsr.JsrSpecWithAlias = jsr.parseJsrPackageName(prefix.slice(0, -'@'.length))
  parsed.spec = spec.spec
  return jsr.createJsrPref(parsed)
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
): string {
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
): string {
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
