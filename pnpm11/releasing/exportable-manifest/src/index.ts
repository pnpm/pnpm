import path from 'node:path'

import { type CatalogResolver, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import type { Hooks } from '@pnpm/hooks.pnpmfile'
import type { Dependencies, ProjectManifest } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'
import { clone, omit } from 'ramda'

import { replaceJsrProtocol } from './jsrProtocol.js'
import { overridePublishConfig } from './overridePublishConfig.js'
import {
  getReadmeRank,
  isPreferredReadme,
  type ReadmeCandidate,
  type ReadmeRank,
  readReadmeFile,
} from './readme.js'
import { type ExportedManifest, transform } from './transform/index.js'
import {
  buildWorkspaceManifestGetter,
  type WorkspacePackageContainer,
  type WorkspacePackageLookup,
} from './workspaceLookup.js'

export { type ExportedManifest }

export {
  getReadmeRank,
  isPreferredReadme,
  type ReadmeCandidate,
  type ReadmeRank,
  readReadmeFile,
  type WorkspacePackageContainer,
  type WorkspacePackageLookup,
}

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
  skipManifestObfuscation?: boolean
  embedReadme?: boolean
  workspacePackages?: WorkspacePackageLookup
}

export async function createExportableManifest (
  dir: string,
  originalManifest: ProjectManifest,
  opts: MakePublishManifestOptions
): Promise<ExportedManifest> {
  const publishManifest = initPublishManifest(originalManifest, opts.skipManifestObfuscation)
  const getWorkspaceManifest = buildWorkspaceManifestGetter(opts.workspacePackages)
  await convertManifestDependencies({
    catalogs: opts.catalogs,
    dir,
    getWorkspaceManifest,
    modulesDir: opts.modulesDir,
    originalManifest,
    publishManifest,
  })

  overridePublishConfig(publishManifest)
  if (publishManifest.readme == null && opts.embedReadme) {
    const readme = await readReadmeFile(dir)
    if (readme != null) {
      publishManifest.readme = readme
    }
  }

  const finalManifest = await applyPublishHooks(publishManifest, dir, opts.hooks?.beforePacking)
  return transform(finalManifest)
}

function initPublishManifest (
  originalManifest: ProjectManifest,
  skipObfuscation?: boolean
): ProjectManifest {
  if (skipObfuscation) {
    return omit(['pnpm' as keyof ProjectManifest], clone(originalManifest))
  }
  const publishManifest: ProjectManifest = omit(['scripts', 'packageManager', 'pnpm' as keyof ProjectManifest], originalManifest) as ProjectManifest
  if (originalManifest.scripts != null) {
    publishManifest.scripts = omit(PREPUBLISH_SCRIPTS, originalManifest.scripts)
  }
  return publishManifest
}

interface ConvertManifestDepsParams {
  catalogs: Catalogs
  dir: string
  getWorkspaceManifest?: (depName: string) => ProjectManifest | undefined
  modulesDir?: string
  originalManifest: ProjectManifest
  publishManifest: ProjectManifest
}

async function convertManifestDependencies (params: ConvertManifestDepsParams): Promise<void> {
  const { catalogs, dir, getWorkspaceManifest, modulesDir, originalManifest, publishManifest } = params
  const replaceCatalogProtocol = resolveCatalogProtocol.bind(null, resolveFromCatalog.bind(null, catalogs))
  const convertDependencyForPublish = combineConverters(replaceCatalogProtocol, replaceWorkspaceProtocol, replaceJsrProtocol)

  await Promise.all((['dependencies', 'devDependencies', 'optionalDependencies'] as const).map(async (depsField) => {
    const deps = await makePublishDependencies(dir, originalManifest[depsField], {
      convertDependencyForPublish,
      modulesDir,
      workspacePackages: getWorkspaceManifest,
    })
    if (deps != null) {
      publishManifest[depsField] = deps
    }
  }))

  const peerDependencies = originalManifest.peerDependencies
  if (peerDependencies) {
    const convertPeersForPublish = combineConverters(replaceCatalogProtocol, replaceWorkspaceProtocolPeerDependency, replaceJsrProtocol)
    publishManifest.peerDependencies = await makePublishDependencies(dir, peerDependencies, {
      convertDependencyForPublish: convertPeersForPublish,
      modulesDir,
      workspacePackages: getWorkspaceManifest,
    })
  }
}

async function applyPublishHooks (
  manifest: ProjectManifest,
  dir: string,
  hooks?: NonNullable<Hooks['beforePacking']>
): Promise<ProjectManifest> {
  let result = manifest
  for (const hook of hooks ?? []) {
    // eslint-disable-next-line no-await-in-loop -- each hook receives the manifest returned by the previous one
    result = await hook(result, dir) ?? result
  }
  return result
}

export type PublishDependencyConverter = (
  depName: string,
  depSpec: string,
  context: PublishDependencyConverterContext
) => Promise<string> | string

export interface PublishDependencyConverterContext {
  dir: string
  modulesDir?: string
  workspacePackages?: (depName: string) => ProjectManifest | undefined
}

function combineConverters (...converters: readonly PublishDependencyConverter[]): PublishDependencyConverter {
  return async (depName, depSpec, context) => {
    let bareSpecifier = depSpec
    for (const converter of converters) {
      // eslint-disable-next-line no-await-in-loop -- each converter receives the specifier returned by the previous one
      bareSpecifier = await converter(depName, bareSpecifier, context)
    }
    return bareSpecifier
  }
}

export interface MakePublishDependenciesOpts {
  readonly modulesDir?: string
  readonly convertDependencyForPublish: PublishDependencyConverter
  readonly workspacePackages?: (depName: string) => ProjectManifest | undefined
}

async function makePublishDependencies (
  dir: string,
  dependencies: Dependencies | undefined,
  { modulesDir, convertDependencyForPublish, workspacePackages }: MakePublishDependenciesOpts
): Promise<Dependencies | undefined> {
  if (dependencies == null) return dependencies
  const publishDependencies = await Promise.all(
    Object.entries(dependencies).map(async ([depName, depSpec]): Promise<[string, string]> =>
      [depName, await convertDependencyForPublish(depName, depSpec, { dir, modulesDir, workspacePackages })]
    )
  )
  return Object.fromEntries(publishDependencies)
}

async function readAndCheckManifest (
  depName: string,
  dependencyDir: string,
  getWorkspaceManifest?: (depName: string) => ProjectManifest | undefined,
  targetPkgName?: string
): Promise<ProjectManifest> {
  const { manifest: dirManifest } = await tryReadProjectManifest(dependencyDir)
  if (dirManifest?.name && dirManifest?.version) {
    return dirManifest
  }
  const lookupName = targetPkgName ?? depName
  const workspaceManifest = getWorkspaceManifest?.(lookupName)
  if (workspaceManifest?.name && workspaceManifest?.version) {
    return workspaceManifest
  }
  return handleMissingOrInvalidManifest(depName, dirManifest ?? undefined, workspaceManifest)
}

function handleMissingOrInvalidManifest (
  depName: string,
  dirManifest?: ProjectManifest,
  workspaceManifest?: ProjectManifest
): never {
  const found = dirManifest?.name || dirManifest?.version
    ? dirManifest
    : workspaceManifest?.name || workspaceManifest?.version
      ? workspaceManifest
      : dirManifest ?? workspaceManifest
  if (found?.name && !found.version) {
    throw new PnpmError(
      'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
      `Cannot resolve workspace protocol of dependency "${depName}" ` +
        'because its package.json has no "version" field.',
      { hint: `Add a "version" field to the package.json of "${found.name}".` }
    )
  }
  if (found) {
    throw new PnpmError(
      'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
      `Cannot resolve workspace protocol of dependency "${depName}" ` +
        'because its package.json has no "name" field.'
    )
  }
  throw new PnpmError(
    'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
    `Cannot resolve workspace protocol of dependency "${depName}" ` +
      'because this dependency is not installed. Try running "pnpm install".'
  )
}

function resolveCatalogProtocol (catalogResolver: CatalogResolver, alias: string, bareSpecifier: string): string {
  const result = catalogResolver({ alias, bareSpecifier })

  switch (result.type) {
    case 'found': return result.resolution.specifier
    case 'unused': return bareSpecifier
    case 'misconfiguration': throw result.error
  }
}

async function replaceWorkspaceProtocol (
  depName: string,
  depSpec: string,
  context: PublishDependencyConverterContext
): Promise<string> {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }

  // Dependencies with bare "*", "^", "~" versions, or no version (workspace:)
  const versionAliasSpecParts = /^workspace:(?:(.+)@)?([\^~*])?$/.exec(depSpec)
  if (versionAliasSpecParts != null) {
    return resolveVersionAliasWorkspaceSpec(depName, versionAliasSpecParts, context)
  }
  if (depSpec.startsWith('workspace:./') || depSpec.startsWith('workspace:../')) {
    return resolveRelativeWorkspaceSpec(depName, depSpec.slice(10), context)
  }
  const spec = depSpec.slice(10)
  return spec.includes('@') ? `npm:${spec}` : spec
}

async function resolveVersionAliasWorkspaceSpec (
  depName: string,
  parts: RegExpExecArray,
  context: PublishDependencyConverterContext
): Promise<string> {
  const modulesDir = context.modulesDir ?? path.join(context.dir, 'node_modules')
  const targetPkgName = parts[1]
  const manifest = await readAndCheckManifest(depName, path.join(modulesDir, depName), context.workspacePackages, targetPkgName)

  const specifierSuffix: string | undefined = parts[2]
  const semverRangeToken = specifierSuffix === '^' || specifierSuffix === '~' ? specifierSuffix : ''
  if (depName !== manifest.name) {
    return `npm:${manifest.name!}@${semverRangeToken}${manifest.version}`
  }
  return `${semverRangeToken}${manifest.version}`
}

async function resolveRelativeWorkspaceSpec (
  depName: string,
  relPath: string,
  context: PublishDependencyConverterContext
): Promise<string> {
  const manifest = await readAndCheckManifest(depName, path.join(context.dir, relPath), context.workspacePackages)
  if (manifest.name === depName) return `${manifest.version}`
  return `npm:${manifest.name}@${manifest.version}`
}

async function replaceWorkspaceProtocolPeerDependency (
  depName: string,
  depSpec: string,
  context: PublishDependencyConverterContext
): Promise<string> {
  if (!depSpec.includes('workspace:')) {
    return depSpec
  }

  const workspaceSpec = WorkspaceSpec.parse(depSpec)
  if (workspaceSpec?.alias != null) {
    const version = workspaceSpec.version === '^' || workspaceSpec.version === '~' || workspaceSpec.version === ''
      ? '*'
      : workspaceSpec.version
    return `npm:${workspaceSpec.alias}@${version}`
  }
  if (workspaceSpec?.version.startsWith('./') || workspaceSpec?.version.startsWith('../')) {
    return replaceWorkspaceProtocol(depName, depSpec, context)
  }

  return resolvePeerWorkspaceSemver(depName, depSpec, context)
}

const WORKSPACE_SEMVER_REGEX = /workspace:([\^~*]|>=|>|<=|<)?((\d+|[xX*])(\.(\d+|[xX*])){0,2})?/

async function resolvePeerWorkspaceSemver (
  depName: string,
  depSpec: string,
  context: PublishDependencyConverterContext
): Promise<string> {
  const versionAliasSpecParts = WORKSPACE_SEMVER_REGEX.exec(depSpec)
  if (versionAliasSpecParts == null) {
    return depSpec.replace('workspace:', '')
  }

  const [, semverRangeGroup = '', version] = versionAliasSpecParts
  if (version) {
    return depSpec.replace('workspace:', '')
  }

  const modulesDir = context.modulesDir ?? path.join(context.dir, 'node_modules')
  const manifest = await readAndCheckManifest(depName, path.join(modulesDir, depName), context.workspacePackages)
  const semverRangeToken = semverRangeGroup !== '*' ? semverRangeGroup : ''

  return depSpec.replace(WORKSPACE_SEMVER_REGEX, `${semverRangeToken}${manifest.version}`)
}
