import fs from 'node:fs'
import path from 'node:path'

import which from 'which'

export interface ExtendPathOptions {
  extraBinPaths?: string[]
  scriptsPrependNodePath?: boolean | 'warn-only'
  log?: {
    warn: (...args: unknown[]) => void
  }
}

let hasWarnedAboutNodePath = false

export function extendPath (wd: string, originalPath: string | undefined, nodeGyp: string, opts: ExtendPathOptions): string {
  const pathArr = [...opts.extraBinPaths ?? []]
  const p = wd.split(/[\\/]node_modules[\\/]/)
  let acc = path.resolve(p.shift()!)

  // we also unshift the bundled node-gyp-bin folder so that
  // the bundled one will be used for installing things.
  pathArr.unshift(nodeGyp)

  p.forEach(pp => {
    pathArr.unshift(path.join(acc, 'node_modules', '.bin'))
    acc = path.join(acc, 'node_modules', pp)
  })
  pathArr.unshift(path.join(acc, 'node_modules', '.bin'))

  if (shouldPrependCurrentNodeDirToPATH(opts)) {
    // prefer current node interpreter in child scripts
    pathArr.push(path.dirname(process.execPath))
  }

  if (originalPath) pathArr.push(originalPath)
  return pathArr.join(process.platform === 'win32' ? ';' : ':')
}

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
