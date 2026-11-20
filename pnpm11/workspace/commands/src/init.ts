import fs from 'node:fs'
import path from 'node:path'

import { packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { isReleaseInstallable, prepareResolvePnpmVersion, type ResolvePnpmVersionOptions } from '@pnpm/engine.pm.commands'
import { PnpmError } from '@pnpm/error'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import type { ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import semver from 'semver'

import { getInitConfig } from './utils.js'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'init-package-manager',
      'init-type',
    ], allTypes),
    bare: Boolean,
  }
}

export const commandNames = ['init']

export function help (): string {
  return renderHelp({
    description: 'Create a package.json file',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Set the module system for the package. Defaults to "module".',
            name: '--init-type <commonjs|module>',
          },
          {
            description: 'Pin the latest pnpm version in package.json, through "devEngines.packageManager" and "packageManager", and auto-download pnpm when it is missing',
            name: '--init-package-manager',
          },
          {
            description: 'Create a package.json file with the bare minimum of required fields',
            name: '--bare',
          },
        ],
      },
    ],
    url: docsUrl('init'),
    usages: ['pnpm init'],
  })
}

export type InitOptions =
  & Pick<ConfigContext, 'cliOptions'>
  & Partial<ResolvePnpmVersionOptions>
  & Partial<Pick<Config,
  | 'initAuthorEmail'
  | 'initAuthorName'
  | 'initAuthorUrl'
  | 'initLicense'
  | 'initPackageManager'
  | 'initType'
  | 'initVersion'
  | 'offline'
  | 'preferOffline'
  | 'workspaceDir'
  >> & {
    bare?: boolean
  }

const MANIFEST_NAMES = ['package.json', 'package.json5', 'package.yaml']

const MANIFEST_FIELD_PRIORITY = Object.fromEntries([
  'name',
  'version',
  'private',
  'description',
  'main',
  'scripts',
  'keywords',
  'author',
  'license',
  'devEngines',
  'packageManager',
].map((key, index) => [key, index]))

export async function handler (opts: InitOptions, params?: string[]): Promise<string> {
  if (params?.length) {
    throw new PnpmError('INIT_ARG', 'init command does not accept any arguments', {
      hint: `Maybe you wanted to run "pnpm create ${params.join(' ')}"`,
    })
  }
  const initDir = opts.cliOptions.dir ?? process.cwd()
  assertNoManifestExists(initDir)

  const packageJson = buildInitialManifest(opts)
  if (shouldPinPackageManager(opts, initDir)) {
    const version = await resolveVersionToPin({ ...opts, dir: initDir })
    applyPackageManagerPin(packageJson, version)
  }

  const sortedPackageJson = sortKeysByPriority({ priority: MANIFEST_FIELD_PRIORITY }, packageJson)
  assertNoManifestExists(initDir)

  const manifestPath = path.join(initDir, 'package.json')
  await writeProjectManifest(manifestPath, sortedPackageJson, { indent: 2 })
  return `Wrote to ${manifestPath}\n\n${JSON.stringify(sortedPackageJson, null, 2)}`
}

function assertNoManifestExists (dir: string): void {
  const existing = MANIFEST_NAMES.find(name => fs.existsSync(path.join(dir, name)))
  if (existing) {
    throw new PnpmError('PACKAGE_JSON_EXISTS', `${existing} already exists`)
  }
}

function buildInitialManifest (opts: InitOptions): ProjectManifest {
  const base: ProjectManifest = opts.bare
    ? {}
    : {
      name: path.basename(process.cwd()),
      version: '1.0.0',
      description: '',
      main: 'index.js',
      scripts: {
        test: 'echo "Error: no test specified" && exit 1',
      },
      keywords: [],
      author: '',
      license: 'ISC',
    }

  if (opts.initType === 'module') {
    base.type = opts.initType
  }
  return { ...base, ...getInitConfig(opts) }
}

function shouldPinPackageManager (opts: InitOptions, initDir: string): boolean {
  if (!opts.initPackageManager) return false
  const isWorkspaceSubpackage = opts.workspaceDir != null &&
    path.resolve(opts.workspaceDir) !== path.resolve(initDir)
  return !isWorkspaceSubpackage
}

function applyPackageManagerPin (packageJson: ProjectManifest, version: string): void {
  packageJson.devEngines = {
    ...packageJson.devEngines,
    packageManager: {
      name: 'pnpm',
      version,
      onFail: 'download',
    },
  }
  packageJson.packageManager = `pnpm@${version}`
}

/**
 * How long each registry request in the `latest` lookup may take. Much
 * shorter than the resolver's usual timeout: the version is a nicety, and a
 * scaffold command that appears to hang is worse than one that pins the
 * running version.
 */
const LATEST_LOOKUP_TIMEOUT = 10000

/**
 * The pnpm version the new project is pinned to: whatever the registry's
 * `latest` tag points at, so a project scaffolded by a long-outdated pnpm
 * does not inherit that staleness through its own pin.
 *
 * Falls back to the running version whenever `latest` cannot be established,
 * and never lets `latest` move the pin backwards — the tag can lag the
 * running version when a new major has shipped without being tagged.
 *
 * The lookup is skipped outright when the caller asked to stay off the
 * network, and when no `cacheDir` was supplied: the CLI always sets one, so
 * its absence marks a programmatic caller that passed nothing to resolve
 * with, which should get the running version rather than a network call it
 * never configured.
 */
async function resolveVersionToPin (opts: InitOptions & Pick<Config, 'dir'>): Promise<string> {
  const { cacheDir } = opts
  if (cacheDir == null || opts.offline === true || opts.preferOffline === true) {
    return packageManager.version
  }
  // Outside the `try`: this reads the settings, so a misconfigured
  // `trustPolicyExclude` fails `pnpm init` the way it fails every other
  // command, rather than being mistaken for an unreachable registry.
  const lookUpLatest = prepareResolvePnpmVersion({
    ...opts,
    cacheDir,
    retry: { retries: 0 },
    timeout: LATEST_LOOKUP_TIMEOUT,
  })
  try {
    const resolved = await lookUpLatest('latest')
    // A `latest` that the maturity or trust policy rejects is not something
    // to pin a new project to, and `pnpm init` has nobody to prompt for
    // approval. A broken release is refused for the reason the pin exists at
    // all: it is shared, so pinning one the running wrapper happens to
    // survive would still break every teammate on the other wrapper.
    if (
      resolved == null ||
      resolved.policyViolation != null ||
      !isReleaseInstallable(resolved.version)
    ) {
      return packageManager.version
    }
    return semver.gt(resolved.version, packageManager.version)
      ? resolved.version
      : packageManager.version
  } catch {
    // Deliberately broad: writing a package.json must not fail because the
    // registry is unreachable, misconfigured, slow, or answering with a
    // version that isn't valid semver. The running version is always a usable
    // pin, so every lookup failure degrades to it.
    return packageManager.version
  }
}
