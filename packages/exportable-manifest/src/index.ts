import path from 'path'
import PnpmError from '@pnpm/error'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { Dependencies, ProjectManifest } from '@pnpm/types'
import fromPairs from 'ramda/src/fromPairs'
import isEmpty from 'ramda/src/isEmpty'
import omit from 'ramda/src/omit'

// property keys that are copied from publishConfig into the manifest
const PUBLISH_CONFIG_WHITELIST = new Set([
  // manifest fields that may make sense to overwrite
  'bin',
  'type',
  'imports',
  // https://github.com/stereobooster/package.json#package-bundlers
  'main',
  'module',
  'typings',
  'types',
  'exports',
  'browser',
  'esnext',
  'es2015',
  'unpkg',
  'umd:main',
  // These are useful to hide in order to avoid warnings during local development
  'os',
  'cpu',
])

const PREPUBLISH_SCRIPTS = [
  'prepublishOnly',
  'prepack',
  'prepare',
  'postpack',
  'publish',
  'postpublish',
]

export default async function makePublishManifest (dir: string, originalManifest: ProjectManifest, opts?: { readmeFile?: string }) {
  const publishManifest: ProjectManifest = omit(['pnpm', 'scripts'], originalManifest)
  if (originalManifest.scripts != null) {
    publishManifest.scripts = omit(PREPUBLISH_SCRIPTS, originalManifest.scripts)
  }
  for (const depsField of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies']) {
    const deps = await makePublishDependencies(dir, originalManifest[depsField])
    if (deps != null) {
      publishManifest[depsField] = deps
    }
  }

  const { publishConfig } = publishManifest
  if (publishConfig != null) {
    Object.keys(publishConfig)
      .filter(key => PUBLISH_CONFIG_WHITELIST.has(key))
      .forEach(key => {
        publishManifest[key] = publishConfig[key]
        delete publishConfig[key]
      })

    if (isEmpty(publishConfig)) {
      delete publishManifest.publishConfig
    }
  }

  if (opts?.readmeFile) {
    publishManifest.readme ??= opts.readmeFile
  }

  return publishManifest
}

async function makePublishDependencies (dir: string, dependencies: Dependencies | undefined) {
  if (dependencies == null) return dependencies
  const publishDependencies: Dependencies = fromPairs(
    await Promise.all(
      Object.entries(dependencies)
        .map(async ([depName, depSpec]) => [
          depName,
          await makePublishDependency(depName, depSpec, dir),
        ])
    ) as any, // eslint-disable-line
  )
  return publishDependencies
}

async function makePublishDependency (depName: string, depSpec: string, dir: string) {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }

  // Dependencies with bare "*", "^" and "~" versions
  const versionAliasSpecParts = /^workspace:([^@]+@)?([\^~*])$/.exec(depSpec)
  if (versionAliasSpecParts != null) {
    const { manifest } = await tryReadProjectManifest(path.join(dir, 'node_modules', depName))
    if ((manifest == null) || !manifest.version) {
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
    const { manifest } = await tryReadProjectManifest(path.join(dir, depSpec.substr(10)))
    if ((manifest == null) || !manifest.name || !manifest.version) {
      throw new PnpmError(
        'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
        `Cannot resolve workspace protocol of dependency "${depName}" ` +
          'because this dependency is not installed. Try running "pnpm install".'
      )
    }
    if (manifest.name === depName) return `${manifest.version}`
    return `npm:${manifest.name}@${manifest.version}`
  }
  depSpec = depSpec.substr(10)
  if (depSpec.includes('@')) {
    return `npm:${depSpec}`
  }
  return depSpec
}
