import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { getObjectValueByPropertyPath, parsePropertyPath } from '@pnpm/object.property-path'
import { readPackageJsonFromDirRawSync } from '@pnpm/pkg-manifest.reader'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  const types = allTypes as Record<string, unknown>
  return {
    dir: types['dir'],
    json: Boolean,
    workspace: [String, Array],
    workspaces: Boolean,
    ws: Boolean,
  }
}

export const commandNames = ['pkg']

export async function handler (
  opts: {
    dir: string
    json?: boolean
    workspace?: string | string[]
    workspaces?: boolean
    ws?: boolean
    workspaceDir?: string
    allProjects?: Array<{ rootDir: string; manifest: Record<string, unknown> }>
    selectedProjectsGraph?: Record<string, { package: { rootDir: string; manifest: Record<string, unknown> } }>
  },
  params: string[]
): Promise<string | void> {
  if (params.length === 0) {
    throw new PnpmError('PKG_MISSING_SUBCOMMAND', 'Missing subcommand', {
      hint: help(),
    })
  }

  if (params[0] === '--help' || params[0] === '-h') {
    return help()
  }

  const [subcmd, ...args] = params

  const workspaces = opts.workspaces || opts.ws
  const workspaceArgs = opts.workspace

  if (workspaces || workspaceArgs) {
    return handleWorkspaceCommand(opts, subcmd, args)
  }

  switch (subcmd) {
    case 'get':
      return get(opts, args)
    case 'set':
      return set(opts, args)
    case 'delete':
      return _delete(opts, args)
    case 'fix':
      return fix(opts)
    default:
      throw new PnpmError('PKG_UNKNOWN_SUBCOMMAND', `Unknown subcommand "${subcmd}"`, {
        hint: help(),
      })
  }
}

async function handleWorkspaceCommand (
  opts: {
    dir: string
    json?: boolean
    workspace?: string | string[]
    workspaces?: boolean
    ws?: boolean
    workspaceDir?: string
    allProjects?: Array<{ rootDir: string; manifest: Record<string, unknown> }>
    selectedProjectsGraph?: Record<string, { package: { rootDir: string; manifest: Record<string, unknown> } }>
  },
  subcmd: string,
  args: string[]
): Promise<string | void> {
  const workspaceDir = opts.workspaceDir
  if (!workspaceDir) {
    throw new PnpmError('PKG_WORKSPACE_NO_ROOT', 'Cannot use workspace options outside of a workspace')
  }

  const selectedProjects = opts.selectedProjectsGraph
    ? Object.values(opts.selectedProjectsGraph)
    : opts.allProjects?.map(p => ({ package: p })) ?? []

  if (selectedProjects.length === 0) {
    throw new PnpmError('PKG_WORKSPACE_NO_PACKAGES', 'No workspace packages found')
  }

  if (subcmd === 'get') {
    const results: Record<string, unknown> = {}
    for (const { package: pkg } of selectedProjects) {
      const manifest = readPackageJsonFromDirRawSync(pkg.rootDir) as unknown as Record<string, unknown>
      const pkgName = String(manifest.name || path.relative(workspaceDir, pkg.rootDir))
      if (args.length === 0) {
        results[pkgName] = manifest
      } else {
        const result: Record<string, unknown> = {}
        for (const key of args) {
          const parsedPath = Array.from(parsePropertyPath(key))
          const value = getObjectValueByPropertyPath(manifest, parsedPath)
          result[key] = value
        }
        results[pkgName] = result
      }
    }
    return JSON.stringify(results, undefined, 2)
  }

  const promises = selectedProjects.map(async ({ package: pkg }) => {
    const pkgOpts = { ...opts, dir: pkg.rootDir }
    switch (subcmd) {
      case 'set':
        return set(pkgOpts, args)
      case 'delete':
        return _delete(pkgOpts, args)
      case 'fix':
        return fix(pkgOpts)
      default:
        throw new PnpmError('PKG_UNKNOWN_SUBCOMMAND', `Unknown subcommand "${subcmd}"`, {
          hint: help(),
        })
    }
  })

  await Promise.all(promises)
}

async function get (opts: { dir: string }, args: string[]): Promise<string> {
  const manifest = readPackageJsonFromDirRawSync(opts.dir) as unknown as Record<string, unknown>

  if (args.length === 0) {
    return JSON.stringify(manifest, undefined, 2)
  }

  const result: Record<string, unknown> = {}
  for (const key of args) {
    const parsedPath = Array.from(parsePropertyPath(key))
    const value = getObjectValueByPropertyPath(manifest, parsedPath)
    result[key] = value
  }

  return JSON.stringify(result, undefined, 2)
}

async function set (
  opts: { dir: string; json?: boolean },
  args: string[]
): Promise<void> {
  if (args.length === 0) {
    throw new PnpmError('PKG_SET_MISSING_ARGS', 'Missing key=value pairs', {
      hint: help(),
    })
  }

  const manifest = readPackageJsonFromDirRawSync(opts.dir) as unknown as Record<string, unknown>
  const manifestPath = path.join(opts.dir, 'package.json')

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
        throw new PnpmError('PKG_SET_JSON_PARSE', `Failed to parse value as JSON: "${value}"`)
      }
    }

    const parsedPath = Array.from(parsePropertyPath(key))
    setObjectValueByPropertyPath(manifest, parsedPath, value)
  }

  await writeProjectManifest(manifestPath, manifest)
}

async function _delete (opts: { dir: string }, args: string[]): Promise<void> {
  if (args.length === 0) {
    throw new PnpmError('PKG_DELETE_MISSING_ARGS', 'Missing keys to delete', {
      hint: help(),
    })
  }

  const manifest = readPackageJsonFromDirRawSync(opts.dir) as unknown as Record<string, unknown>
  const manifestPath = path.join(opts.dir, 'package.json')

  for (const key of args) {
    const parsedPath = Array.from(parsePropertyPath(key))
    deleteObjectValueByPropertyPath(manifest, parsedPath)
  }

  await writeProjectManifest(manifestPath, manifest)
}

async function fix (opts: { dir: string }): Promise<void> {
  const manifest = readPackageJsonFromDirRawSync(opts.dir) as unknown as Record<string, unknown>
  const manifestPath = path.join(opts.dir, 'package.json')

  const originalManifest = JSON.parse(JSON.stringify(manifest))

  if (manifest.name != null && typeof manifest.name !== 'string') {
    delete manifest.name
  }

  if (manifest.version != null && typeof manifest.version !== 'string') {
    delete manifest.version
  }

  if (manifest.dependencies != null && typeof manifest.dependencies !== 'object') {
    delete manifest.dependencies
  }
  if (manifest.devDependencies != null && typeof manifest.devDependencies !== 'object') {
    delete manifest.devDependencies
  }
  if (manifest.optionalDependencies != null && typeof manifest.optionalDependencies !== 'object') {
    delete manifest.optionalDependencies
  }
  if (manifest.peerDependencies != null && typeof manifest.peerDependencies !== 'object') {
    delete manifest.peerDependencies
  }

  if (manifest.scripts != null && typeof manifest.scripts !== 'object') {
    delete manifest.scripts
  }

  if (manifest.bin != null && typeof manifest.bin !== 'string' && typeof manifest.bin !== 'object') {
    delete manifest.bin
  }

  if (JSON.stringify(originalManifest) !== JSON.stringify(manifest)) {
    await writeProjectManifest(manifestPath, manifest)
  }
}

type ObjectOrArray = Record<string | number, unknown> | unknown[]

function setObjectValueByPropertyPath (obj: ObjectOrArray, path: (string | number | { type: 'empty-bracket' })[], value: unknown): void {
  for (let i = 0; i < path.length - 1; i++) {
    const key = path[i]

    if (typeof key === 'object' && 'type' in key && key.type === 'empty-bracket') {
      if (!Array.isArray(obj)) {
        obj = [] as unknown[]
      }
      ;(obj as unknown[]).push({})
      obj = (obj as unknown[])[(obj as unknown[]).length - 1] as ObjectOrArray
      continue
    }

    if (
      typeof obj !== 'object' ||
      obj === null ||
      (!Object.hasOwn(obj, key as string | number) && !(Array.isArray(obj) && typeof key === 'number'))
    ) {
      if (Array.isArray(obj)) {
        (obj as unknown[])[key as number] = typeof path[i + 1] === 'number' ? [] : {}
      } else {
        (obj as Record<string | number, unknown>)[key as string | number] = typeof path[i + 1] === 'number' ? [] : {}
      }
    }
    obj = (obj as Record<string | number, unknown>)[key as string | number] as ObjectOrArray
  }

  const lastKey = path[path.length - 1]
  if (typeof lastKey === 'object' && 'type' in lastKey && lastKey.type === 'empty-bracket') {
    if (!Array.isArray(obj)) {
      throw new PnpmError('PKG_SET_INVALID_PATH', 'Cannot use [] on non-array')
    }
    (obj as unknown[]).push(value)
  } else {
    (obj as Record<string | number, unknown>)[lastKey as string | number] = value
  }
}

function deleteObjectValueByPropertyPath (obj: ObjectOrArray, path: (string | number)[]): void {
  for (let i = 0; i < path.length - 1; i++) {
    const key = path[i]
    if (
      typeof obj !== 'object' ||
      obj === null ||
      !Object.hasOwn(obj, key) ||
      (Array.isArray(obj) && typeof key !== 'number')
    ) {
      return
    }
    obj = (obj as Record<string | number, unknown>)[key] as ObjectOrArray
  }

  const lastKey = path[path.length - 1]
  delete (obj as Record<string | number, unknown>)[lastKey]
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
            description: 'Parse values as JSON when setting',
            name: '--json',
          },
          {
            description: 'Run in specific workspace packages',
            name: '--workspace <name>',
          },
          {
            description: 'Run in all workspace packages',
            name: '--workspaces, -w',
          },
        ],
      },
    ],
    url: docsUrl('cli/pkg'),
    usages: [
      'pnpm pkg get [<key> [<key> ...]]',
      'pnpm pkg set <key>=<value> [<key>=<value> ...]',
      'pnpm pkg delete <key> [<key> ...]',
      'pnpm pkg fix',
      'pnpm pkg set <key>=<value> --json',
      'pnpm pkg get name --workspace packages',
      'pnpm pkg set version=1.0.0 --workspaces',
    ],
  })
}
