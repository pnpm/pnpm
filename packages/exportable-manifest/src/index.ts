import PnpmError from '@pnpm/error'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { Dependencies, ProjectManifest } from '@pnpm/types'
import path = require('path')
import R = require('ramda')

// property keys that are copied from publishConfig into the manifest
const PUBLISH_CONFIG_WHITELIST = new Set([
  // manifest fields that may make sense to overwrite
  'bin',
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
])

export default async function makePublishManifest (dir: string, originalManifest: ProjectManifest) {
  const publishManifest = {
    ...originalManifest,
  }
  for (const depsField of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies']) {
    const deps = await makePublishDependencies(dir, originalManifest[depsField])
    if (deps) {
      publishManifest[depsField] = deps
    }
  }

  const { publishConfig } = originalManifest
  if (publishConfig) {
    Object.keys(publishConfig)
      .filter(key => PUBLISH_CONFIG_WHITELIST.has(key))
      .forEach(key => {
        publishManifest[key] = publishConfig[key]
      })
  }

  return publishManifest
}

async function makePublishDependencies (dir: string, dependencies: Dependencies | undefined) {
  if (!dependencies) return dependencies
  const publishDependencies: Dependencies = R.fromPairs(
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
  if (depSpec === 'workspace:*' || depSpec.endsWith('@*')) {
    const { manifest } = await tryReadProjectManifest(path.join(dir, 'node_modules', depName))
    if (!manifest || !manifest.version) {
      throw new PnpmError(
        'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
        `Cannot resolve workspace protocol of dependency "${depName}" ` +
          'because this dependency is not installed. Try running "pnpm install".'
      )
    }
    if (depName !== manifest.name) {
      return `npm:${manifest.name!}@${manifest.version}`
    }
    return manifest.version
  }
  if (depSpec.startsWith('workspace:./') || depSpec.startsWith('workspace:../')) {
    const { manifest } = await tryReadProjectManifest(path.join(dir, depSpec.substr(10)))
    if (!manifest || !manifest.name || !manifest.version) {
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
