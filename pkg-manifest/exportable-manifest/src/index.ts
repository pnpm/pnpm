import path from 'path'
import { type CatalogResolver, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { type Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import { parseJsrSpecifier } from '@pnpm/resolving.jsr-specifier-parser'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { type Hooks } from '@pnpm/pnpmfile'
import { type Dependencies, type ProjectManifest } from '@pnpm/types'
import { omit } from 'ramda'
import pMapValues from 'p-map-values'
import { overridePublishConfig } from './overridePublishConfig.js'
import { type ExportedManifest, transform } from './transform/index.js'

export { type ExportedManifest }

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
  hooks?: Hooks
  modulesDir?: string
  readmeFile?: string
}

export async function createExportableManifest (
  dir: string,
  originalManifest: ProjectManifest,
  opts: MakePublishManifestOptions
): Promise<ExportedManifest> {
  let publishManifest: ProjectManifest = omit(['pnpm', 'scripts', 'packageManager'], originalManifest)
  if (originalManifest.scripts != null) {
    publishManifest.scripts = omit(PREPUBLISH_SCRIPTS, originalManifest.scripts)
  }

  const catalogResolver = resolveFromCatalog.bind(null, opts.catalogs)
  const replaceCatalogProtocol = resolveCatalogProtocol.bind(null, catalogResolver)

  const convertDependencyForPublish = combineConverters(replaceWorkspaceProtocol, replaceCatalogProtocol, replaceJsrProtocol)
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
    const convertPeersForPublish = combineConverters(replaceWorkspaceProtocolPeerDependency, replaceCatalogProtocol, replaceJsrProtocol)
    publishManifest.peerDependencies = await makePublishDependencies(dir, peerDependencies, {
      modulesDir: opts?.modulesDir,
      convertDependencyForPublish: convertPeersForPublish,
    })
  }

  overridePublishConfig(publishManifest)

  if (opts?.readmeFile) {
    publishManifest.readme ??= opts.readmeFile
  }

  for (const hook of opts?.hooks?.beforePacking ?? []) {
    // eslint-disable-next-line no-await-in-loop
    publishManifest = await hook(publishManifest, dir) ?? publishManifest
  }

  return transform(publishManifest)
}

export type PublishDependencyConverter = (
  depName: string,
  depSpec: string,
  dir: string,
  modulesDir?: string
) => Promise<string> | string

function combineConverters (...converters: readonly PublishDependencyConverter[]): PublishDependencyConverter {
  return async (depName, depSpec, dir, modulesDir) => {
    let bareSpecifier = depSpec
    for (const converter of converters) {
      // eslint-disable-next-line no-await-in-loop
      bareSpecifier = await converter(depName, bareSpecifier, dir, modulesDir)
    }
    return bareSpecifier
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
  const publishDependencies = await pMapValues.default(
    async (depSpec, depName) => convertDependencyForPublish(depName, depSpec, dir, modulesDir),
    dependencies
  )
  return publishDependencies
}

async function readAndCheckManifest (depName: string, dependencyDir: string): Promise<ProjectManifest> {
  const { manifest } = await tryReadProjectManifest(dependencyDir)
  if (!manifest?.name || !manifest?.version) {
    throw new PnpmError(
      'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
      `Cannot resolve workspace protocol of dependency "${depName}" ` +
        'because this dependency is not installed. Try running "pnpm install".'
    )
  }
  return manifest
}

function resolveCatalogProtocol (catalogResolver: CatalogResolver, alias: string, bareSpecifier: string): string {
  const result = catalogResolver({ alias, bareSpecifier })

  switch (result.type) {
  case 'found': return result.resolution.specifier
  case 'unused': return bareSpecifier
  case 'misconfiguration': throw result.error
  }
}

async function replaceWorkspaceProtocol (depName: string, depSpec: string, dir: string, modulesDir?: string): Promise<string> {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }

  // Dependencies with bare "*", "^", "~" versions, or no version (workspace:)
  const versionAliasSpecParts = /^workspace:(?:(.+)@)?([\^~*])?$/.exec(depSpec)
  if (versionAliasSpecParts != null) {
    modulesDir = modulesDir ?? path.join(dir, 'node_modules')
    const manifest = await readAndCheckManifest(depName, path.join(modulesDir, depName))

    const specifierSuffix: string | undefined = versionAliasSpecParts[2]
    const semverRangeToken = specifierSuffix === '^' || specifierSuffix === '~' ? specifierSuffix : ''
    if (depName !== manifest.name) {
      return `npm:${manifest.name!}@${semverRangeToken}${manifest.version}`
    }
    return `${semverRangeToken}${manifest.version}`
  }
  if (depSpec.startsWith('workspace:./') || depSpec.startsWith('workspace:../')) {
    const manifest = await readAndCheckManifest(depName, path.join(dir, depSpec.slice(10)))

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

  // Dependencies with bare "*", "^", "~",">=",">","<=", "<", version
  const workspaceSemverRegex = /workspace:([\^~*]|>=|>|<=|<)?((\d+|[xX*])(\.(\d+|[xX*])){0,2})?/
  const versionAliasSpecParts = workspaceSemverRegex.exec(depSpec)

  if (versionAliasSpecParts != null) {
    const [, semverRangGroup = '', version] = versionAliasSpecParts

    if (version) {
      return depSpec.replace('workspace:', '')
    }

    modulesDir = modulesDir ?? path.join(dir, 'node_modules')
    const manifest = await readAndCheckManifest(depName, path.join(modulesDir, depName))
    const semverRangeToken = semverRangGroup !== '*' ? semverRangGroup : ''

    return depSpec.replace(workspaceSemverRegex, `${semverRangeToken}${manifest.version}`)
  }

  return depSpec.replace('workspace:', '')
}

async function replaceJsrProtocol (depName: string, depSpec: string): Promise<string> {
  const spec = parseJsrSpecifier(depSpec, depName)
  if (spec == null) {
    return depSpec
  }
  return createNpmAliasedSpecifier(spec.npmPkgName, spec.versionSelector)
}

function createNpmAliasedSpecifier (npmPkgName: string, versionSelector?: string): string {
  const npmPkgSpecifier = `npm:${npmPkgName}`
  if (!versionSelector) {
    return npmPkgSpecifier
  }
  return `${npmPkgSpecifier}@${versionSelector}`
}
