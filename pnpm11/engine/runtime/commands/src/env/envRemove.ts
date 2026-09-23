import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { globalWarn } from '@pnpm/logger'

import type { NvmNodeCommandOptions } from './node.js'

export async function envRemove (opts: NvmNodeCommandOptions, params: string[]): Promise<void> {
  globalWarn('"pnpm env remove" is deprecated. Use "pnpm remove -g node" instead.')
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env remove <version>" can only be used with the "--global" option currently')
  }

  const version = params[0]?.trim()
  if (!version) {
    throw new PnpmError('MISSING_NODE_VERSION', '"pnpm env remove --global <version>" requires a Node.js version to be specified')
  }

  let removed = false

  // 1. Try removing global node package via pnpm CLI
  try {
    const args = ['remove', '--global', 'node']
    if (opts.bin) args.push('--global-bin-dir', opts.bin)
    if (opts.storeDir) args.push('--store-dir', opts.storeDir)
    if (opts.cacheDir) args.push('--cache-dir', opts.cacheDir)
    runPnpmCli(args, { cwd: opts.pnpmHomeDir })
    removed = true
  } catch {
    // If 'node' wasn't installed as a global package, proceed to check legacy/dangling links
  }

  // 2. Check and clean up legacy directories in pnpmHomeDir
  const removedNames = new Set<string>([version])
  if (opts.pnpmHomeDir) {
    const nodejsDir = path.join(opts.pnpmHomeDir, 'nodejs')
    if (fs.existsSync(nodejsDir)) {
      try {
        const entries = fs.readdirSync(nodejsDir)
        const entriesToRemove = entries.filter((entry) =>
          entry === version || entry.startsWith(`${version}.`) || entry.startsWith(version)
        )
        if (entriesToRemove.length > 0) {
          await Promise.all(
            entriesToRemove.map(async (entry) => {
              await fs.promises.rm(path.join(nodejsDir, entry), { recursive: true, force: true })
              removedNames.add(entry)
            })
          )
          removed = true
        }
      } catch {}
    }

    const nodeCurrentLink = path.join(opts.pnpmHomeDir, 'nodejs_current')
    try {
      const stat = fs.lstatSync(nodeCurrentLink)
      if (stat.isSymbolicLink()) {
        const target = fs.readlinkSync(nodeCurrentLink)
        const isDangling = !fs.existsSync(nodeCurrentLink)
        const pointsToRemoved = Array.from(removedNames).some((name) => target.includes(name))
        if (isDangling || pointsToRemoved) {
          await fs.promises.unlink(nodeCurrentLink)
          removed = true
        }
      }
    } catch {}
  }

  // 3. Check and clean up bin links in opts.bin if dangling or pointing to the removed version
  if (opts.bin) {
    const unlinks: Array<Promise<void>> = []
    for (const binBase of ['node', 'npm', 'npx']) {
      const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1', '.exe'] : ['']
      for (const ext of extensions) {
        const binFile = path.join(opts.bin, `${binBase}${ext}`)
        try {
          const stat = fs.lstatSync(binFile)
          if (stat.isSymbolicLink()) {
            const target = fs.readlinkSync(binFile)
            const isDangling = !fs.existsSync(binFile)
            const pointsToRemoved = Array.from(removedNames).some((name) => target.includes(name)) ||
              target.includes('nodejs')
            if (isDangling || pointsToRemoved) {
              unlinks.push(fs.promises.unlink(binFile))
              removed = true
            }
          }
        } catch {}
      }
    }
    await Promise.all(unlinks)
  }

  if (!removed) {
    throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js version matching ${version}`)
  }
}
