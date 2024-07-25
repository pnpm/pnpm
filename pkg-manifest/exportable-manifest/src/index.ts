import path from 'path'
import { type CatalogResolver, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { type Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { type Dependencies, type ProjectManifest } from '@pnpm/types'
import omit from 'ramda/src/omit'
import pMapValues from 'p-map-values'
import { overridePublishConfig } from './overridePublishConfig'

const PREPUBLISH_SCRIPTS = [
  'prepublishOnly',
  'prepack',
  'prepare',
  'postpack',
  'publish',
  'postpublish',
]

export interface MakePublishManifestOptions {
  catalogs: Catalogs
  modulesDir?: string
  readmeFile?: string
}

export async function createExportableManifest (
  dir: string,
  originalManifest: ProjectManifest,
  opts: MakePublishManifestOptions
): Promise<ProjectManifest> {
  const publishManifest: ProjectManifest = omit(['pnpm', 'scripts', 'packageManager'], originalManifest)
  if (originalManifest.scripts != null) {
    publishManifest.scripts = omit(PREPUBLISH_SCRIPTS, originalManifest.scripts)
  }

  const catalogResolver = resolveFromCatalog.bind(null, opts.catalogs)
  const replaceCatalogProtocol = resolveCatalogProtocol.bind(null, catalogResolver)

  const convertDependencyForPublish = combineConverters(replaceWorkspaceProtocol, replaceCatalogProtocol)
  await Promise.all((['dependencies', 'devDependencies', 'optionalDependencies'] as const).map(async (depsField) => {
    const deps = await makePublishDependencies(dir, originalManifest[depsField], {
      modulesDir: opts?.modulesDir,
      convertDependencyForPublish,
    })
    if (deps != null) {
      publishManifest[depsField] = deps
    }
  }))

  const peerDependencies = originalManifest.peerDependencies
  if (peerDependencies) {
    const convertPeersForPublish = combineConverters(replaceWorkspaceProtocolPeerDependency, replaceCatalogProtocol)
    publishManifest.peerDependencies = await makePublishDependencies(dir, peerDependencies, {
      modulesDir: opts?.modulesDir,
      convertDependencyForPublish: convertPeersForPublish,
    })
  }

  overridePublishConfig(publishManifest)

  if (opts?.readmeFile) {
    publishManifest.readme ??= opts.readmeFile
  }

  return publishManifest
}

export type PublishDependencyConverter = (
  depName: string,
  depSpec: string,
  dir: string,
  modulesDir?: string
) => Promise<string> | string

function combineConverters (...converters: readonly PublishDependencyConverter[]): PublishDependencyConverter {
  return async (depName, depSpec, dir, modulesDir) => {
    let pref = depSpec
    for (const converter of converters) {
      // eslint-disable-next-line no-await-in-loop
      pref = await converter(depName, pref, dir, modulesDir)
    }
    return pref
  }
}

export interface MakePublishDependenciesOpts {
  readonly modulesDir?: string
  readonly convertDependencyForPublish: PublishDependencyConverter
}

async function makePublishDependencies (
  dir: string,
  dependencies: Dependencies | undefined,
  { modulesDir, convertDependencyForPublish }: MakePublishDependenciesOpts
): Promise<Dependencies | undefined> {
  if (dependencies == null) return dependencies
  const publishDependencies = await pMapValues(
    async (depSpec, depName) => convertDependencyForPublish(depName, depSpec, dir, modulesDir),
    dependencies
  )
  return publishDependencies
}

async function resolveManifest (depName: string, modulesDir: string): Promise<ProjectManifest> {
  const { manifest } = await tryReadProjectManifest(path.join(modulesDir, depName))
  if (!manifest?.name || !manifest?.version) {
    throw new PnpmError(
      'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
      `Cannot resolve workspace protocol of dependency "${depName}" ` +
        'because this dependency is not installed. Try running "pnpm install".'
    )
  }

  return manifest
}

function resolveCatalogProtocol (catalogResolver: CatalogResolver, alias: string, pref: string): string {
  const result = catalogResolver({ alias, pref })

  switch (result.type) {
  case 'found': return result.resolution.specifier
  case 'unused': return pref
  case 'misconfiguration': throw result.error
  }
}

async function replaceWorkspaceProtocol (depName: string, depSpec: string, dir: string, modulesDir?: string): Promise<string> {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }

  // Dependencies with bare "*", "^" and "~" versions
  const versionAliasSpecParts = /^workspace:(.*?)@?([\^~*])$/.exec(depSpec)
  if (versionAliasSpecParts != null) {
    modulesDir = modulesDir ?? path.join(dir, 'node_modules')
    const manifest = await resolveManifest(depName, modulesDir)

    const semverRangeToken = versionAliasSpecParts[2] !== '*' ? versionAliasSpecParts[2] : ''
    if (depName !== manifest.name) {
      return `npm:${manifest.name!}@${semverRangeToken}${manifest.version}`
    }
    return `${semverRangeToken}${manifest.version}`
  }
  if (depSpec.startsWith('workspace:./') || depSpec.startsWith('workspace:../')) {
    const manifest = await resolveManifest(depName, path.join(dir, depSpec.slice(10)))

    if (manifest.name === depName) return `${manifest.version}`
    return `npm:${manifest.name}@${manifest.version}`
  }
  depSpec = depSpec.slice(10)
  if (depSpec.includes('@')) {
    return `npm:${depSpec}`
  }
  return depSpec
}

async function replaceWorkspaceProtocolPeerDependency (depName: string, depSpec: string, dir: string, modulesDir?: string) {
  if (!depSpec.includes('workspace:')) {
    return depSpec
  }

  // Dependencies with bare "*", "^", "~",">=",">","<=",< versions
  const workspaceSemverRegex = /workspace:([\^~*]|>=|>|<=|<)/
  const versionAliasSpecParts = workspaceSemverRegex.exec(depSpec)

  if (versionAliasSpecParts != null) {
    modulesDir = modulesDir ?? path.join(dir, 'node_modules')
    const manifest = await resolveManifest(depName, modulesDir)

    const [,semverRangGroup] = versionAliasSpecParts

    const semverRangeToken = semverRangGroup !== '*' ? semverRangGroup : ''

    return depSpec.replace(workspaceSemverRegex, `${semverRangeToken}${manifest.version}`)
  }

  depSpec = depSpec.replace('workspace:', '')

  return depSpec
}
