import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { type CatalogResolver, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import type { Hooks } from '@pnpm/hooks.pnpmfile'
import { parseJsrSpecifier } from '@pnpm/resolving.jsr-specifier-parser'
import type { Dependencies, ProjectManifest } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'
import { clone, omit } from 'ramda'

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

export type WorkspacePackageLookup =
  | Array<{ manifest: ProjectManifest, rootDir?: string }>
  | Map<string, unknown>
  | Record<string, unknown>
  | ((depName: string) => ProjectManifest | undefined)

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
  let publishManifest: ProjectManifest
  if (opts.skipManifestObfuscation) {
    publishManifest = omit(['pnpm' as keyof ProjectManifest], clone(originalManifest))
  } else {
    publishManifest = omit(['scripts', 'packageManager', 'pnpm' as keyof ProjectManifest], originalManifest)
    if (originalManifest.scripts != null) {
      publishManifest.scripts = omit(PREPUBLISH_SCRIPTS, originalManifest.scripts)
    }
  }

  const catalogResolver = resolveFromCatalog.bind(null, opts.catalogs)
  const replaceCatalogProtocol = resolveCatalogProtocol.bind(null, catalogResolver)

  const getWorkspaceManifest = buildWorkspaceManifestGetter(opts.workspacePackages)
  const convertDependencyForPublish = combineConverters(replaceCatalogProtocol, replaceWorkspaceProtocol, replaceJsrProtocol)
  await Promise.all((['dependencies', 'devDependencies', 'optionalDependencies'] as const).map(async (depsField) => {
    const deps = await makePublishDependencies(dir, originalManifest[depsField], {
      modulesDir: opts?.modulesDir,
      convertDependencyForPublish,
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
      modulesDir: opts?.modulesDir,
      convertDependencyForPublish: convertPeersForPublish,
      workspacePackages: getWorkspaceManifest,
    })
  }

  overridePublishConfig(publishManifest)

  if (publishManifest.readme == null && opts?.embedReadme) {
    const readme = await readReadmeFile(dir)
    if (readme != null) {
      publishManifest.readme = readme
    }
  }

  for (const hook of opts?.hooks?.beforePacking ?? []) {
    // eslint-disable-next-line no-await-in-loop
    publishManifest = await hook(publishManifest, dir) ?? publishManifest
  }

  return transform(publishManifest)
}

// `O_NOFOLLOW` makes the open itself refuse a symlink at the final path component, closing the
// TOCTOU window between the `readdir` type check and the read: a symlink swapped in for README.md
// after the check can't redirect the read outside the project and leak its target into the
// published manifest. Windows lacks the flag (and requires privileges to create symlinks), so it
// falls back to a plain read.
const README_READ_FLAGS = fs.constants.O_RDONLY | (process.platform === 'win32' ? 0 : fs.constants.O_NOFOLLOW)

export async function readReadmeFile (projectDir: string): Promise<string | undefined> {
  const entries = await fs.promises.readdir(projectDir, { withFileTypes: true })
  // Only embed a regular README.md file — a symlink is skipped (see README_READ_FLAGS).
  const readmeEntry = entries.find((entry) => entry.isFile() && /^readme\.md$/i.test(entry.name))
  if (readmeEntry == null) return undefined
  let handle: fs.promises.FileHandle | undefined
  try {
    handle = await fs.promises.open(path.join(projectDir, readmeEntry.name), README_READ_FLAGS)
    return await handle.readFile('utf8')
  } catch (err: unknown) {
    // ELOOP: the entry is a symlink after all (a concurrent swap) — skip it, as the isFile() check intended.
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ELOOP') return undefined
    throw err
  } finally {
    await handle?.close()
  }
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
      // eslint-disable-next-line no-await-in-loop
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

function extractManifest (val: unknown): ProjectManifest | undefined {
  if (!val || typeof val !== 'object') return undefined
  if ('manifest' in val && val.manifest && typeof val.manifest === 'object') {
    return val.manifest as ProjectManifest
  }
  if ('package' in val && val.package && typeof val.package === 'object' && 'manifest' in val.package && val.package.manifest && typeof val.package.manifest === 'object') {
    return val.package.manifest as ProjectManifest
  }
  if ('name' in val && typeof (val as Record<string, unknown>).name === 'string') {
    return val as ProjectManifest
  }
  return undefined
}

function buildWorkspaceManifestGetter (
  workspacePackages?: WorkspacePackageLookup
): ((depName: string) => ProjectManifest | undefined) | undefined {
  if (!workspacePackages) return undefined
  if (typeof workspacePackages === 'function') {
    return workspacePackages
  }
  const byName = new Map<string, ProjectManifest>()
  const addManifest = (val: unknown) => {
    const manifest = extractManifest(val)
    // Keep name-only manifests so a missing version is reported as such
    // instead of as "not installed".
    if (manifest?.name && !byName.has(manifest.name)) {
      byName.set(manifest.name, manifest)
    }
  }

  if (Array.isArray(workspacePackages)) {
    for (const pkg of workspacePackages) {
      addManifest(pkg)
    }
  } else if (workspacePackages instanceof Map) {
    for (const val of workspacePackages.values()) {
      if (val instanceof Map) {
        for (const subVal of val.values()) {
          addManifest(subVal)
        }
      } else {
        addManifest(val)
      }
    }
  } else if (typeof workspacePackages === 'object') {
    for (const val of Object.values(workspacePackages)) {
      addManifest(val)
    }
  }
  return (depName: string) => byName.get(depName)
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
  { dir, modulesDir, workspacePackages }: PublishDependencyConverterContext
): Promise<string> {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }

  // Dependencies with bare "*", "^", "~" versions, or no version (workspace:)
  const versionAliasSpecParts = /^workspace:(?:(.+)@)?([\^~*])?$/.exec(depSpec)
  if (versionAliasSpecParts != null) {
    modulesDir = modulesDir ?? path.join(dir, 'node_modules')
    const targetPkgName = versionAliasSpecParts[1]
    const manifest = await readAndCheckManifest(depName, path.join(modulesDir, depName), workspacePackages, targetPkgName)

    const specifierSuffix: string | undefined = versionAliasSpecParts[2]
    const semverRangeToken = specifierSuffix === '^' || specifierSuffix === '~' ? specifierSuffix : ''
    if (depName !== manifest.name) {
      return `npm:${manifest.name!}@${semverRangeToken}${manifest.version}`
    }
    return `${semverRangeToken}${manifest.version}`
  }
  if (depSpec.startsWith('workspace:./') || depSpec.startsWith('workspace:../')) {
    const manifest = await readAndCheckManifest(depName, path.join(dir, depSpec.slice(10)), workspacePackages)

    if (manifest.name === depName) return `${manifest.version}`
    return `npm:${manifest.name}@${manifest.version}`
  }
  depSpec = depSpec.slice(10)
  if (depSpec.includes('@')) {
    return `npm:${depSpec}`
  }
  return depSpec
}

async function replaceWorkspaceProtocolPeerDependency (
  depName: string,
  depSpec: string,
  { dir, modulesDir, workspacePackages }: PublishDependencyConverterContext
) {
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
    return replaceWorkspaceProtocol(depName, depSpec, { dir, modulesDir, workspacePackages })
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
    const manifest = await readAndCheckManifest(depName, path.join(modulesDir, depName), workspacePackages)
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
