import { promises as fs } from 'node:fs'
import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir } from 'is-subdir'
import { pathAbsolute } from 'path-absolute'
import { pathExists } from 'path-exists'
import { renderHelp } from 'render-help'

export const commandNames = ['clean', 'purge']

export const overridableByScript = true

export const rcOptionsTypes = (): Record<string, unknown> => ({})

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    lockfile: Boolean,
  }
}

export const shorthands: Record<string, string> = {
  l: '--lockfile',
}

export function help (): string {
  return renderHelp({
    aliases: ['purge'],
    description: 'Safely remove node_modules directories from all workspace projects. \
Uses Node.js to remove directories, which correctly handles NTFS junctions on Windows \
without following them into their targets. \
If the current project has a "clean" (or "purge") script in package.json, \
the script is executed instead of the built-in command.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Also remove pnpm-lock.yaml files',
            name: '--lockfile',
            shortAlias: '-l',
          },
        ],
      },
    ],
    url: docsUrl('clean'),
    usages: ['pnpm clean [--lockfile]'],
  })
}

export async function handler (
  opts: {
    dir: string
    lockfile?: boolean
    modulesDir?: string
    virtualStoreDir?: string
    workspaceDir?: string
    workspacePackagePatterns?: string[]
    cliOptions?: {
      lockfile?: boolean
    }
  }
): Promise<void> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const rootDir = opts.workspaceDir ?? opts.dir
  const cleanOpts = { modulesDir, removeLockfile: opts.cliOptions?.lockfile === true }
  const dirs = await getProjectDirs(opts)
  await Promise.all(dirs.map(cleanProjectDir.bind(null, cleanOpts)))
  if (opts.virtualStoreDir) {
    // virtualStoreDir is resolved relative to the project/workspace root,
    // matching how pnpm resolves it in get-context (via pathAbsolute).
    // The default 'node_modules/.pnpm' is inside node_modules and already
    // cleaned above, so we only need to handle the case where it's outside.
    const resolvedVirtualStoreDir = path.isAbsolute(opts.virtualStoreDir)
      ? opts.virtualStoreDir
      : path.resolve(rootDir, opts.virtualStoreDir)
    const rootModulesDir = pathAbsolute(modulesDir, rootDir)
    if (
      !isSubdir(rootModulesDir, resolvedVirtualStoreDir) &&
      isSubdir(rootDir, resolvedVirtualStoreDir) &&
      await pathExists(resolvedVirtualStoreDir)
    ) {
      printRemoving(resolvedVirtualStoreDir)
      await rimraf(resolvedVirtualStoreDir)
    }
  }
}

function printRemoving (p: string): void {
  process.stdout.write(`Removing ${path.relative(process.cwd(), p) || '.'}\n`)
}

async function cleanProjectDir (opts: { modulesDir: string, removeLockfile?: boolean }, dir: string): Promise<void> {
  const fullModulesDir = pathAbsolute(opts.modulesDir, dir)
  if (await hasContentsToRemove(fullModulesDir)) {
    printRemoving(fullModulesDir)
    await removeModulesDirContents(fullModulesDir)
  }
  if (opts.removeLockfile) {
    const lockfilePath = path.join(dir, 'pnpm-lock.yaml')
    if (await pathExists(lockfilePath)) {
      printRemoving(lockfilePath)
      await rimraf(lockfilePath)
    }
  }
}

const PNPM_HIDDEN_ENTRIES = new Set(['.bin', '.modules.yaml', '.pnpm', '.pnpm-workspace-state-v1.json'])

async function hasContentsToRemove (modulesDir: string): Promise<boolean> {
  let items: string[]
  try {
    items = await fs.readdir(modulesDir)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return false
    throw err
  }
  return items.some((item) => item[0] !== '.' || PNPM_HIDDEN_ENTRIES.has(item))
}

async function removeModulesDirContents (modulesDir: string): Promise<void> {
  let items: string[]
  try {
    items = await fs.readdir(modulesDir)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return
    throw err
  }
  await Promise.all(items.map(async (item) => {
    if (item[0] === '.' && !PNPM_HIDDEN_ENTRIES.has(item)) return
    await rimraf(path.join(modulesDir, item))
  }))
}

async function getProjectDirs (
  opts: {
    dir: string
    workspaceDir?: string
    workspacePackagePatterns?: string[]
  }
): Promise<string[]> {
  if (!opts.workspaceDir) {
    return [opts.dir]
  }
  const pkgs = await findWorkspaceProjectsNoCheck(opts.workspaceDir, {
    patterns: opts.workspacePackagePatterns,
  })
  return pkgs.map((pkg) => pkg.rootDir)
}
