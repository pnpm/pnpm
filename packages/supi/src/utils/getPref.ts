import PnpmError from '@pnpm/error'
import { ResolvedDirectDependency } from '@pnpm/resolve-dependencies'
import versionSelectorType = require('version-selector-type')
import { ImporterToUpdate } from '../install'
import { PinnedVersion } from '../install/getWantedDependencies'
import save, { PackageSpecObject } from '../save'

export async function updateProjectManifest (
  importer: ImporterToUpdate,
  opts: {
    directDependencies: ResolvedDirectDependency[],
    preserveWorkspaceProtocol: boolean,
    saveWorkspaceProtocol: boolean,
  }
) {
  if (!importer.manifest) {
    throw new Error('Cannot save because no package.json found')
  }
  const specsToUpsert = opts.directDependencies
    .filter((rdd, index) => importer.wantedDependencies[index]!.updateSpec)
    .map((rdd, index) => {
      const wantedDep = importer.wantedDependencies[index]!
      return resolvedDirectDepToSpecObject({ ...rdd, isNew: wantedDep.isNew, specRaw: wantedDep.raw }, importer, {
        pinnedVersion: wantedDep.pinnedVersion ?? importer['pinnedVersion'] ?? 'major',
        preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
        saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      })
    })
  for (const pkgToInstall of importer.wantedDependencies) {
    if (pkgToInstall.updateSpec && pkgToInstall.alias && !specsToUpsert.some(({ alias }) => alias === pkgToInstall.alias)) {
      specsToUpsert.push({
        alias: pkgToInstall.alias,
        peer: importer['peer'],
        saveType: importer['targetDependenciesField'],
      })
    }
  }
  const hookedManifest = await save(
    importer.rootDir,
    importer.manifest,
    specsToUpsert,
    { dryRun: true }
  )
  const originalManifest = importer.originalManifest && await save(
    importer.rootDir,
    importer.originalManifest,
    specsToUpsert,
    { dryRun: true }
  )
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
  }: ResolvedDirectDependency & { isNew?: Boolean, specRaw: string },
  importer: ImporterToUpdate,
  opts: {
    pinnedVersion: PinnedVersion,
    preserveWorkspaceProtocol: boolean,
    saveWorkspaceProtocol: boolean,
  }
): PackageSpecObject {
  let pref!: string
  if (normalizedPref) {
    pref = normalizedPref
  } else {
    if (isNew) {
      pref = getPrefPreferSpecifiedSpec({
        alias,
        name,
        pinnedVersion: opts.pinnedVersion,
        specRaw,
        version,
      })
    } else {
      pref = getPrefPreferSpecifiedExoticSpec({
        alias,
        name,
        pinnedVersion: opts.pinnedVersion,
        specRaw,
        version,
      })
    }
    if (
      resolution.type === 'directory' &&
      (opts.saveWorkspaceProtocol || opts.preserveWorkspaceProtocol !== false && specRaw.includes('@workspace:')) &&
      !pref.startsWith('workspace:')
    ) {
      pref = `workspace:${pref}`
    }
  }
  return {
    alias,
    peer: importer['peer'],
    pref,
    saveType: isNew ? importer['targetDependenciesField'] : undefined,
  }
}

const getPrefix = (alias: string, name: string) => alias !== name ? `npm:${name}@` : ''

export default function getPref (
  alias: string,
  name: string,
  version: string,
  opts: {
    pinnedVersion?: PinnedVersion,
  }
) {
  const prefix = getPrefix(alias, name)
  return `${prefix}${createVersionSpec(version, opts.pinnedVersion)}`
}

function getPrefPreferSpecifiedSpec (
  opts: {
    alias: string,
    name: string
    version: string,
    specRaw: string,
    pinnedVersion?: PinnedVersion,
  }
 ) {
  const prefix = getPrefix(opts.alias, opts.name)
  if (opts.specRaw?.startsWith(`${opts.alias}@${prefix}`)) {
    const range = opts.specRaw.substr(`${opts.alias}@${prefix}`.length)
    if (range) {
      const selector = versionSelectorType(range)
      if (selector && (selector.type === 'version' || selector.type === 'range')) {
        return opts.specRaw.substr(opts.alias.length + 1)
      }
    }
  }
  return `${prefix}${createVersionSpec(opts.version, opts.pinnedVersion)}`
}

function getPrefPreferSpecifiedExoticSpec (
  opts: {
    alias: string,
    name: string
    version: string,
    specRaw: string,
    pinnedVersion: PinnedVersion,
  }
) {
  const prefix = getPrefix(opts.alias, opts.name)
  if (opts.specRaw?.startsWith(`${opts.alias}@${prefix}`) && opts.specRaw !== `${opts.alias}@workspace:*`) {
    const specWithoutName = opts.specRaw.substr(`${opts.alias}@${prefix}`.length)
    const selector = versionSelectorType(specWithoutName)
    if (!(selector && (selector.type === 'version' || selector.type === 'range'))) {
      return opts.specRaw.substr(opts.alias.length + 1)
    }
  }
  return `${prefix}${createVersionSpec(opts.version, opts.pinnedVersion)}`
}

function createVersionSpec (version: string, pinnedVersion?: PinnedVersion) {
  switch (pinnedVersion || 'major') {
    case 'none':
      return '*'
    case 'major':
      return `^${version}`
    case 'minor':
      return `~${version}`
    case 'patch':
      return `${version}`
    default:
      throw new PnpmError('BAD_PINNED_VERSION', `Cannot pin '${pinnedVersion}'`)
  }
}
