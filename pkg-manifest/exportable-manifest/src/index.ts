import path from 'path'
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
  modulesDir?: string
  readmeFile?: string
}

export async function createExportableManifest (
  dir: string,
  originalManifest: ProjectManifest,
  opts?: MakePublishManifestOptions
): Promise<ProjectManifest> {
  const publishManifest: ProjectManifest = omit(['pnpm', 'scripts', 'packageManager'], originalManifest)
  if (originalManifest.scripts != null) {
    publishManifest.scripts = omit(PREPUBLISH_SCRIPTS, originalManifest.scripts)
  }
  await Promise.all((['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies'] as const).map(async (depsField) => {
    const deps = await makePublishDependencies(dir, originalManifest[depsField], opts?.modulesDir)
    if (deps != null) {
      publishManifest[depsField] = deps
    }
  }))

  overridePublishConfig(publishManifest)

  if (opts?.readmeFile) {
    publishManifest.readme ??= opts.readmeFile
  }

  return publishManifest
}

async function makePublishDependencies (
  dir: string,
  dependencies: Dependencies | undefined,
  modulesDir?: string
): Promise<Dependencies | undefined> {
  if (dependencies == null) return dependencies
  const publishDependencies = await pMapValues(
    (depSpec, depName) => makePublishDependency(depName, depSpec, dir, modulesDir),
    dependencies
  )
  return publishDependencies
}

async function makePublishDependency (depName: string, depSpec: string, dir: string, modulesDir?: string): Promise<string> {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }

  // Dependencies with bare "*", "^" and "~" versions
  const versionAliasSpecParts = /^workspace:(.*?)@?([\^~*])$/.exec(depSpec)
  if (versionAliasSpecParts != null) {
    modulesDir = modulesDir ?? path.join(dir, 'node_modules')
    const { manifest } = await tryReadProjectManifest(path.join(modulesDir, depName))
    if (!manifest?.version) {
      throw new PnpmError(
        'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
        `Cannot resolve workspace protocol of dependency "${depName}" ` +
          'because this dependency is not installed. Try running "pnpm install".'
      )
    }

    const semverRangeToken = versionAliasSpecParts[2] !== '*' ? versionAliasSpecParts[2] : ''
    if (depName !== manifest.name) {
      return `npm:${manifest.name!}@${semverRangeToken}${manifest.version}`
    }
    return `${semverRangeToken}${manifest.version}`
  }
  if (depSpec.startsWith('workspace:./') || depSpec.startsWith('workspace:../')) {
    const { manifest } = await tryReadProjectManifest(path.join(dir, depSpec.slice(10)))
    if (!manifest?.name || !manifest?.version) {
      throw new PnpmError(
        'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
        `Cannot resolve workspace protocol of dependency "${depName}" ` +
          'because this dependency is not installed. Try running "pnpm install".'
      )
    }
    if (manifest.name === depName) return `${manifest.version}`
    return `npm:${manifest.name}@${manifest.version}`
  }
  depSpec = depSpec.slice(10)
  if (depSpec.includes('@')) {
    return `npm:${depSpec}`
  }
  return depSpec
}
