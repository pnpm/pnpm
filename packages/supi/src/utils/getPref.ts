import PnpmError from '@pnpm/error'
import { ResolvedDirectDependency } from '@pnpm/resolve-dependencies'
import versionSelectorType = require('version-selector-type')
import { ImporterToUpdate } from '../install'
import { PinnedVersion } from '../install/getWantedDependencies'
import save, { PackageSpecObject } from '../save'

export async function updateImporterManifest (
  importer: ImporterToUpdate,
  opts: {
    directDependencies: ResolvedDirectDependency[],
    saveWorkspaceProtocol: boolean,
  },
) {
  if (!importer.manifest) {
    throw new Error('Cannot save because no package.json found')
  }
  const specsToUpsert = opts.directDependencies.map((rdd, index) => resolvedDirectDepToSpecObject(rdd, importer, {
    pinnedVersion: importer.wantedDependencies[index]?.pinnedVersion ?? importer['pinnedVersion'] ?? 'major',
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
  }))
  for (const pkgToInstall of importer.wantedDependencies) {
    if (pkgToInstall.alias && !specsToUpsert.some(({ alias }) => alias === pkgToInstall.alias)) {
      specsToUpsert.push({
        alias: pkgToInstall.alias,
        peer: importer['peer'],
        saveType: importer['targetDependenciesField'],
      })
    }
  }
  return save(
    importer.rootDir,
    importer.manifest,
    specsToUpsert,
    { dryRun: true },
  )
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
  }: ResolvedDirectDependency,
  importer: ImporterToUpdate,
  opts: {
    pinnedVersion: PinnedVersion,
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
      opts.saveWorkspaceProtocol &&
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
  },
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
  },
 ) {
  const prefix = getPrefix(opts.alias, opts.name)
  if (opts.specRaw?.startsWith(`${opts.alias}@${prefix}`)) {
    const selector = versionSelectorType(opts.specRaw.substr(`${opts.alias}@${prefix}`.length))
    if (selector && (selector.type === 'version' || selector.type === 'range')) {
      return opts.specRaw.substr(opts.alias.length + 1)
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
  },
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
