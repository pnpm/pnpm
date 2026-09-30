import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

/**
 * Find the nearest directory containing package.json, node_modules,
 * or pnpm-workspace.yaml by walking up from startDir.
 * Ported from @pnpm/npm-conf/lib/util.js findPrefix.
 */
export function findLocalPrefix (startDir: string): string {
  let name = path.resolve(startDir)

  let walkedUp = false
  while (path.basename(name) === 'node_modules') {
    name = path.dirname(name)
    walkedUp = true
  }

  if (walkedUp) {
    return name
  }

  return findPrefixUp(name, name)
}

function findPrefixUp (name: string, original: string): string {
  if (isFilesystemRoot(name)) {
    return original
  }

  try {
    const files = fs.readdirSync(name)
    if (PREFIX_MARKER_FILES.some((file) => files.includes(file))) {
      return name
    }

    const dirname = path.dirname(name)
    if (dirname === name) {
      return original
    }

    return findPrefixUp(dirname, original)
  } catch (err: unknown) {
    if (name === original && !(isError(err) && 'code' in err && err.code === 'ENOENT')) {
      throw err
    }
    return original
  }
}

const PREFIX_MARKER_FILES = ['node_modules', 'package.json', 'package.json5', 'package.yaml', 'pnpm-workspace.yaml']

function isFilesystemRoot (dir: string): boolean {
  const driveRootRegex = /^[a-z]:[/\\]?$/i
  return dir === '/' || (process.platform === 'win32' && driveRootRegex.test(dir))
}
