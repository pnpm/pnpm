import type { ProjectManifest } from '@pnpm/types'
import { isEmpty } from 'ramda'

// property keys that are copied from publishConfig into the manifest
const PUBLISH_CONFIG_WHITELIST = new Set([
  // Lets a package be published under a name it cannot carry in the workspace
  // — typically because a sibling project already owns that name. Nothing else
  // in the workspace observes the rename: dependents, pnpm-lock.yaml, and the
  // changeset ledger all keep addressing the project by its manifest name.
  'name',
  // manifest fields that may make sense to overwrite
  'bin',
  'engines',
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
  'libc',
  // https://www.typescriptlang.org/docs/handbook/declaration-files/publishing.html#version-selection-with-typesversions
  'typesVersions',
])

export function overridePublishConfig (publishManifest: ProjectManifest): void {
  const { publishConfig } = publishManifest
  if (!publishConfig) return

  for (const key in publishConfig) {
    if (!PUBLISH_CONFIG_WHITELIST.has(key)) continue
    const value = publishConfig[key]
    publishManifest[key as keyof ProjectManifest] = value as any // eslint-disable-line @typescript-eslint/no-explicit-any
    delete publishConfig[key]
  }

  if (isEmpty(publishConfig)) {
    delete publishManifest.publishConfig
  }
}
