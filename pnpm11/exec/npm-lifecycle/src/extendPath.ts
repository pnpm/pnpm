import fs from 'node:fs'
import path from 'node:path'

import which from 'which'

export interface ExtendPathOptions {
  /** The directory of the `node-gyp` wrappers, placed after every `node_modules/.bin`. */
  nodeGypBinDir: string
  /**
   * The `.bin` that holds `wd`'s own executables, used in place of
   * `<wd>/node_modules/.bin` when `modulesDir` puts them elsewhere. Only the
   * entry for `wd` itself changes; the packages above it keep their
   * `node_modules/.bin`, which is where their own dependencies are installed.
   */
  wdBinDir?: string
  extraBinPaths?: string[]
  scriptsPrependNodePath?: boolean | 'warn-only'
  log?: {
    warn: (...args: unknown[]) => void
  }
}

/**
 * Builds the `PATH` of a script running in `wd`: the bin directory of `wd`
 * and the `node_modules/.bin` of every package above it, the `node-gyp`
 * wrappers, the extra bin directories, and then `originalPath` without the
 * entries already listed before it. A script that runs `pnpm run` would
 * otherwise inherit a `PATH` that gains another copy of them at every level.
 */
export function extendPath (wd: string, originalPath: string | undefined, opts: ExtendPathOptions): string {
  const pathArr = [...opts.extraBinPaths ?? []]
  const p = wd.split(/[\\/]node_modules[\\/]/)
  let acc = path.resolve(p.shift()!)

  // we also unshift the bundled node-gyp-bin folder so that
  // the bundled one will be used for installing things.
  pathArr.unshift(opts.nodeGypBinDir)

  p.forEach(pp => {
    pathArr.unshift(path.join(acc, 'node_modules', '.bin'))
    acc = path.join(acc, 'node_modules', pp)
  })
  pathArr.unshift(opts.wdBinDir ?? path.join(acc, 'node_modules', '.bin'))

  if (shouldPrependCurrentNodeDirToPATH(opts)) {
    // prefer current node interpreter in child scripts
    pathArr.push(path.dirname(process.execPath))
  }

  const delimiter = process.platform === 'win32' ? ';' : ':'
  if (originalPath) {
    const added = new Set(pathArr)
    pathArr.push(...originalPath.split(delimiter).filter(entry => !added.has(entry)))
  }
  return pathArr.join(delimiter)
}

let hasWarnedAboutNodePath = false

function shouldPrependCurrentNodeDirToPATH (opts: ExtendPathOptions): boolean {
  const setting = opts.scriptsPrependNodePath
  if (setting === false || setting == null) return false
  if (setting === true) return true

  let isDifferentNodeInPath: boolean

  const isWindows = process.platform === 'win32'
  let foundExecPath: string | undefined
  try {
    foundExecPath = which.sync(path.basename(process.execPath), { pathExt: isWindows ? ';' : ':' })
    // Apply `fs.realpath()` here to avoid false positives when `node` is a symlinked executable.
    isDifferentNodeInPath = fs.realpathSync(process.execPath).toUpperCase() !==
        fs.realpathSync(foundExecPath).toUpperCase()
  } catch {
    isDifferentNodeInPath = true
  }

  if (setting === 'warn-only') {
    if (isDifferentNodeInPath && !hasWarnedAboutNodePath) {
      if (foundExecPath) {
        opts.log?.warn('lifecycle', `The node binary used for scripts is ${foundExecPath} but pnpm is using ${process.execPath} itself. Use the \`--scripts-prepend-node-path\` option to include the path for the node binary pnpm was executed with.`)
      } else {
        opts.log?.warn('lifecycle', `pnpm is using ${process.execPath} but there is no node binary in the current PATH. Use the \`--scripts-prepend-node-path\` option to include the path for the node binary pnpm was executed with.`)
      }
      hasWarnedAboutNodePath = true
    }

    return false
  }

  return isDifferentNodeInPath
}
