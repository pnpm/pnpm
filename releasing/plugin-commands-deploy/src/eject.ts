import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import { isEmptyDirOrNothing } from '@pnpm/fs.is-empty-dir-or-nothing'
import { install } from '@pnpm/plugin-commands-installation'
import rimraf from '@zkochan/rimraf'
import renderHelp from 'render-help'
import { type ProjectManifest } from '@pnpm/types'
import { logger } from '@pnpm/logger'

export const shorthands = {
  ...install.shorthands,
}

// For while, no especific options from eject
const EJECT_OWN_OPTIONS = {}

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...install.rcOptionsTypes(),
    ...EJECT_OWN_OPTIONS,
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...install.cliOptionsTypes(),
    ...EJECT_OWN_OPTIONS,
  }
}

export const commandNames = ['eject']

export function help (): string {
  return renderHelp({
    description: 'Eject specific dependencies to an isolated node_modules directory',
    url: docsUrl('eject'),
    usages: ['pnpm eject <target directory>'],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: "Packages in `devDependencies` won't be installed",
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Only `devDependencies` are installed',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: '`optionalDependencies` are not installed',
            name: '--no-optional',
          },
        ],
      },
      {
        title: 'Details',
        list: [
          {
            name: '',
            description: 'Dependencies marked with "ejected: true" in dependenciesMeta will be installed to the target directory.',
          },
        ],
      },
    ],
  })
}

export type EjectOptions = Omit<install.InstallCommandOptions, 'useLockfile'>

export async function handler (opts: EjectOptions, params: string[]): Promise<void> {
  if (params.length !== 1) {
    throw new PnpmError('INVALID_EJECT_TARGET', 'This command requires one parameter: the target directory')
  }

  const targetDirParam = params[0]
  const targetDir = path.isAbsolute(targetDirParam)
    ? targetDirParam
    : path.join(opts.dir, targetDirParam)

  const manifestPath = path.join(opts.dir, 'package.json')
  let manifest: ProjectManifest

  try {
    const manifestContent = await fs.promises.readFile(manifestPath, 'utf-8')
    manifest = JSON.parse(manifestContent)
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  } catch (error) {
    throw new PnpmError('MANIFEST_NOT_FOUND', `Could not read package.json at ${manifestPath}`)
  }

  const ejectedDeps = extractEjectedDependencies(manifest)

  if (Object.keys(ejectedDeps).length === 0) {
    throw new PnpmError(
      'NO_EJECTED_DEPENDENCIES',
      'No dependencies marked with "ejected: true" found in dependenciesMeta.\n\n' +
      'Example package.json:\n' +
      '{\n' +
      '  "dependencies": {\n' +
      '    "argon2": "^0.44.0"\n' +
      '  },\n' +
      '  "dependenciesMeta": {\n' +
      '    "argon2": { "ejected": true }\n' +
      '  }\n' +
      '}'
    )
  }

  if (!isEmptyDirOrNothing(targetDir)) {
    if (!opts.force) {
      throw new PnpmError('EJECT_DIR_NOT_EMPTY', `Target directory ${targetDir} is not empty. Use --force to overwrite.`)
    }
    logger.warn({ message: 'using --force, deleting target directory', prefix: targetDir })
  }

  await rimraf(targetDir)
  await fs.promises.mkdir(targetDir, { recursive: true })

  const ejectedManifest: ProjectManifest = {
    name: manifest.name ? `${manifest.name}-ejected` : 'ejected-dependencies',
    version: manifest.version || '1.0.0',
    private: true,
  }

  if (ejectedDeps.dependencies && Object.keys(ejectedDeps.dependencies).length > 0) {
    ejectedManifest.dependencies = ejectedDeps.dependencies
  }
  if (ejectedDeps.devDependencies && Object.keys(ejectedDeps.devDependencies).length > 0) {
    ejectedManifest.devDependencies = ejectedDeps.devDependencies
  }
  if (ejectedDeps.optionalDependencies && Object.keys(ejectedDeps.optionalDependencies).length > 0) {
    ejectedManifest.optionalDependencies = ejectedDeps.optionalDependencies
  }

  const ejectedManifestPath = path.join(targetDir, 'package.json')
  await fs.promises.writeFile(
    ejectedManifestPath,
    JSON.stringify(ejectedManifest, null, 2) + '\n'
  )

  logger.info({
    message: `Ejecting ${Object.keys(ejectedDeps).length} dependencies to ${targetDir}`,
    prefix: opts.dir,
  })

  await install.handler({
    ...opts,
    dir: targetDir,
    lockfileDir: targetDir,
    rootProjectManifest: ejectedManifest,
    rootProjectManifestDir: targetDir,
    workspaceDir: undefined, // Not a workspace
    allProjects: undefined,
    allProjectsGraph: undefined,
    selectedProjectsGraph: undefined,
    confirmModulesPurge: false,
    depth: Infinity,
    frozenLockfile: false,
    preferFrozenLockfile: false,
    saveLockfile: true, // Creates lockfile on target
    virtualStoreDir: path.join(targetDir, 'node_modules/.pnpm'),
    modulesDir: 'node_modules',
    rawLocalConfig: {
      ...opts.rawLocalConfig,
      'frozen-lockfile': false,
    },
  })

  logger.info({
    message: `Successfully ejected dependencies to ${targetDir}`,
    prefix: opts.dir,
  })
}

interface EjectedDependencies {
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
}

function extractEjectedDependencies (manifest: ProjectManifest): EjectedDependencies {
  const result: EjectedDependencies = {}
  const dependenciesMeta = manifest.dependenciesMeta || {}

  const depFields = ['dependencies', 'devDependencies', 'optionalDependencies'] as const

  for (const field of depFields) {
    const deps = manifest[field]
    if (!deps) continue

    for (const [pkgName, version] of Object.entries(deps)) {
      const meta = dependenciesMeta[pkgName]

      if (meta && meta.ejected === true) {
        if (!result[field]) {
          result[field] = {}
        }
        result[field]![pkgName] = version
      }
    }
  }

  return result
}
