import fs from 'node:fs/promises'
import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { docsUrl, readProjectManifest, readProjectManifestOnly } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import type { Hooks } from '@pnpm/hooks.pnpmfile'
import {
  deleteObjectValueByPropertyPathString,
  getObjectValueByPropertyPathString,
  setObjectValueByPropertyPathString,
} from '@pnpm/object.property-path'
import { createExportableManifest } from '@pnpm/releasing.exportable-manifest'
import type { ProjectManifest } from '@pnpm/types'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  const types = allTypes as Record<string, unknown>
  return {
    dir: types['dir'],
    json: Boolean,
    recursive: Boolean,
  }
}

export const commandNames = ['pkg']

interface PkgCommandOptions {
  dir: string
  json?: boolean
  recursive?: boolean
  catalogs?: Catalogs
  hooks?: Hooks
  embedReadme?: boolean
  skipManifestObfuscation?: boolean
  workspaceDir?: string
  selectedProjectsGraph?: Record<string, { package: { rootDir: string, manifest: Record<string, unknown> } }>
}

export async function handler (opts: PkgCommandOptions, params: string[]): Promise<string | void> {
  if (params.length === 0) {
    throw new PnpmError('PKG_MISSING_SUBCOMMAND', 'Missing subcommand', {
      hint: help(),
    })
  }

  if (params[0] === '--help' || params[0] === '-h') {
    return help()
  }

  const [subcmd, ...args] = params

  if (opts.recursive) {
    return handleRecursiveCommand(opts, subcmd, args)
  }

  return runSubcommand(opts, subcmd, args)
}

async function runSubcommand (opts: PkgCommandOptions, subcmd: string, args: string[]): Promise<string | void> {
  switch (subcmd) {
    case 'get':
      return pkgGet(opts, args)
    case 'get-published':
      return pkgGetPublished(opts, args)
    case 'set':
      return pkgSet(opts, args)
    case 'delete':
      return pkgDelete(opts, args)
    case 'fix':
      return pkgFix(opts)
    default:
      throw new PnpmError('PKG_UNKNOWN_SUBCOMMAND', `Unknown subcommand "${subcmd}"`, {
        hint: help(),
      })
  }
}

async function handleRecursiveCommand (opts: PkgCommandOptions, subcmd: string, args: string[]): Promise<string | void> {
  const workspaceDir = opts.workspaceDir
  if (!workspaceDir) {
    throw new PnpmError('PKG_RECURSIVE_NO_ROOT', 'Cannot run recursively outside of a workspace')
  }

  const selectedProjects = opts.selectedProjectsGraph == null
    ? []
    : Object.values(opts.selectedProjectsGraph)

  if (selectedProjects.length === 0) {
    throw new PnpmError('PKG_RECURSIVE_NO_PACKAGES', 'No workspace packages were selected')
  }

  if (subcmd === 'get' || subcmd === 'get-published') {
    const readFn = subcmd === 'get-published' ? readPublishedManifest : readRawManifest
    const entries = await Promise.all(selectedProjects.map(async ({ package: pkg }) => {
      const manifest = await readFn({ ...opts, dir: pkg.rootDir })
      const pkgName = String(manifest.name ?? path.relative(workspaceDir, pkg.rootDir))
      return [pkgName, selectFromManifest(manifest, args)] as const
    }))
    return JSON.stringify(Object.fromEntries(entries), undefined, 2)
  }

  await Promise.all(selectedProjects.map(({ package: pkg }) =>
    runSubcommand({ ...opts, dir: pkg.rootDir }, subcmd, args)
  ))
}

async function pkgGet (opts: PkgCommandOptions, args: string[]): Promise<string> {
  return formatManifestFields(await readRawManifest(opts), args, opts)
}

async function pkgGetPublished (opts: PkgCommandOptions, args: string[]): Promise<string> {
  return formatManifestFields(await readPublishedManifest(opts), args, opts)
}

function formatManifestFields (manifest: Record<string, unknown>, args: string[], opts: PkgCommandOptions): string {
  if (args.length === 1) {
    const value = getObjectValueByPropertyPathString(manifest, args[0])
    if (value === undefined) return ''
    if (opts.json) return JSON.stringify(value, undefined, 2)
    return typeof value === 'string' ? value : JSON.stringify(value, undefined, 2)
  }

  return JSON.stringify(selectFromManifest(manifest, args), undefined, 2)
}

async function readRawManifest (opts: PkgCommandOptions): Promise<Record<string, unknown>> {
  return await readProjectManifestOnly(opts.dir) as Record<string, unknown>
}

async function readPublishedManifest (opts: PkgCommandOptions): Promise<Record<string, unknown>> {
  const manifest = await readProjectManifestOnly(opts.dir) as ProjectManifest
  const dir = await resolvePublishDir(opts.dir, manifest)
  const sourceManifest = dir !== opts.dir
    ? await readProjectManifestOnly(dir) as ProjectManifest
    : manifest
  return await createExportableManifest(dir, sourceManifest, {
    catalogs: opts.catalogs ?? {},
    hooks: opts.hooks,
    embedReadme: opts.embedReadme,
    modulesDir: path.join(opts.dir, 'node_modules'),
    skipManifestObfuscation: opts.skipManifestObfuscation,
  }) as unknown as Record<string, unknown>
}

async function resolvePublishDir (projectDir: string, manifest: ProjectManifest): Promise<string> {
  if (!manifest.publishConfig?.directory) return projectDir
  const resolved = path.resolve(projectDir, manifest.publishConfig.directory)
  let real: string
  try {
    real = await fs.realpath(resolved)
  } catch {
    real = resolved
  }
  if (!real.startsWith(projectDir + path.sep) && real !== projectDir) {
    throw new PnpmError(
      'PUBLISH_DIR_OUTSIDE_PROJECT',
      `publishConfig.directory "${manifest.publishConfig.directory}" resolves outside the project`
    )
  }
  return real
}

function selectFromManifest (manifest: Record<string, unknown>, args: string[]): unknown {
  if (args.length === 0) return manifest
  const result: Record<string, unknown> = {}
  for (const key of args) {
    result[key] = getObjectValueByPropertyPathString(manifest, key)
  }
  return result
}

async function pkgSet (opts: PkgCommandOptions, args: string[]): Promise<void> {
  if (args.length === 0) {
    throw new PnpmError('PKG_SET_MISSING_ARGS', 'Missing key=value pairs', {
      hint: help(),
    })
  }

  const { manifest, writeProjectManifest } = await readProjectManifest(opts.dir)

  for (const arg of args) {
    const eqIndex = arg.indexOf('=')
    if (eqIndex === -1) {
      throw new PnpmError('PKG_SET_INVALID_ARG', `Invalid argument "${arg}". Expected key=value format`, {
        hint: 'Example: pnpm pkg set name=my-package',
      })
    }

    const key = arg.slice(0, eqIndex)
    let value: unknown = arg.slice(eqIndex + 1)

    if (opts.json) {
      try {
        value = JSON.parse(value as string)
      } catch {
        throw new PnpmError('PKG_SET_JSON_PARSE', `Failed to parse value as JSON: "${value as string}"`)
      }
    }

    setObjectValueByPropertyPathString(manifest as unknown as Record<string, unknown>, key, value)
  }

  await writeProjectManifest(manifest)
}

async function pkgDelete (opts: PkgCommandOptions, args: string[]): Promise<void> {
  if (args.length === 0) {
    throw new PnpmError('PKG_DELETE_MISSING_ARGS', 'Missing keys to delete', {
      hint: help(),
    })
  }

  const { manifest, writeProjectManifest } = await readProjectManifest(opts.dir)

  for (const key of args) {
    deleteObjectValueByPropertyPathString(manifest as unknown as Record<string, unknown>, key)
  }

  await writeProjectManifest(manifest)
}

async function pkgFix (opts: PkgCommandOptions): Promise<void> {
  const { manifest, writeProjectManifest } = await readProjectManifest(opts.dir)
  const m = manifest as ProjectManifest & Record<string, unknown>

  if ('name' in m && typeof m.name !== 'string') {
    delete m.name
  }

  if ('version' in m && typeof m.version !== 'string') {
    delete m.version
  }

  for (const field of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies', 'scripts'] as const) {
    if (field in m && !isPlainObject(m[field])) {
      delete m[field]
    }
  }

  if ('bin' in m && typeof m.bin !== 'string' && !isPlainObject(m.bin)) {
    delete m.bin
  }

  await writeProjectManifest(manifest)
}

function isPlainObject (value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export function help (): string {
  return renderHelp({
    description: 'Manages your package.json',
    descriptionLists: [
      {
        title: 'Commands',
        list: [
          {
            description: 'Retrieves a value from package.json',
            name: 'get [<key> [<key> ...]]',
          },
          {
            description: 'Retrieves a value from the publish-transformed package.json (with publishConfig overrides, workspace protocol resolution, script stripping, etc. applied)',
            name: 'get-published [<key> [<key> ...]]',
          },
          {
            description: 'Sets a value in package.json',
            name: 'set <key>=<value> [<key>=<value> ...]',
          },
          {
            description: 'Deletes a key from package.json',
            name: 'delete <key> [<key> ...]',
          },
          {
            description: 'Auto corrects common errors in package.json',
            name: 'fix',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'When setting, parse the value as JSON. When getting a single key, return its JSON-encoded form instead of the raw value',
            name: '--json',
          },
          {
            description: 'Run on every workspace project or every project selected by a filter',
            name: '--recursive',
            shortAlias: '-r',
          },
        ],
      },
    ],
    url: docsUrl('pkg'),
    usages: [
      'pnpm pkg get [<key> [<key> ...]]',
      'pnpm pkg get-published [<key> [<key> ...]]',
      'pnpm pkg set <key>=<value> [<key>=<value> ...]',
      'pnpm pkg delete <key> [<key> ...]',
      'pnpm pkg fix',
      'pnpm pkg set <key>=<value> --json',
      'pnpm -r pkg get name',
      'pnpm -r pkg get-published exports',
      'pnpm --filter <selector> pkg get name',
      'pnpm -r pkg set version=1.0.0',
    ],
  })
}
