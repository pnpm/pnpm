import { promises as fs } from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { findWorkspacePackagesNoCheck } from '@pnpm/workspace.find-packages'
import isSubdir from 'is-subdir'
import rimraf from '@zkochan/rimraf'
import renderHelp from 'render-help'

export const commandNames = ['clean']

export const rcOptionsTypes = cliOptionsTypes

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
    description: 'Safely remove node_modules directories from all workspace projects. \
Uses Node.js to remove directories, which correctly handles NTFS junctions on Windows \
without following them into their targets.',
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
  }
): Promise<void> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const rootDir = opts.workspaceDir ?? opts.dir
  const dirs = await getProjectDirs(opts)
  await Promise.all(dirs.map((dir) => cleanProjectDir(dir, modulesDir, opts.lockfile)))
  if (opts.virtualStoreDir) {
    // virtualStoreDir is resolved relative to the project/workspace root,
    // matching how pnpm resolves it in get-context (via pathAbsolute).
    // The default 'node_modules/.pnpm' is inside node_modules and already
    // cleaned above, so we only need to handle the case where it's outside.
    const resolvedVirtualStoreDir = path.isAbsolute(opts.virtualStoreDir)
      ? opts.virtualStoreDir
      : path.resolve(rootDir, opts.virtualStoreDir)
    const rootModulesDir = path.join(rootDir, modulesDir)
    if (!isSubdir(rootModulesDir, resolvedVirtualStoreDir) && isSubdir(rootDir, resolvedVirtualStoreDir)) {
      try {
        await fs.access(resolvedVirtualStoreDir)
      } catch (err: unknown) {
        if ((err as NodeJS.ErrnoException).code === 'ENOENT') return
        throw err
      }
      printRemoving(resolvedVirtualStoreDir)
      await rimraf(resolvedVirtualStoreDir)
    }
  }
}

function printRemoving (p: string): void {
  process.stdout.write(`Removing ${path.relative(process.cwd(), p) || '.'}\n`)
}

async function cleanProjectDir (dir: string, modulesDir: string, lockfile?: boolean): Promise<void> {
  const fullModulesDir = path.join(dir, modulesDir)
  if (await hasContentsToRemove(fullModulesDir)) {
    printRemoving(fullModulesDir)
    await removeModulesDirContents(fullModulesDir)
  }
  if (lockfile) {
    const lockfilePath = path.join(dir, 'pnpm-lock.yaml')
    try {
      await fs.access(lockfilePath)
    } catch (err: unknown) {
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') return
      throw err
    }
    printRemoving(lockfilePath)
    await rimraf(lockfilePath)
  }
}

const PNPM_HIDDEN_ENTRIES = new Set(['.bin', '.modules.yaml', '.pnpm'])

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
  const items = await fs.readdir(modulesDir)
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
  const pkgs = await findWorkspacePackagesNoCheck(opts.workspaceDir, {
    patterns: opts.workspacePackagePatterns,
  })
  return pkgs.map((pkg) => pkg.rootDir)
}
