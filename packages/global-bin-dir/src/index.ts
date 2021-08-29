import fs from 'fs'
import path from 'path'
import PnpmError from '@pnpm/error'
import { sync as canWriteToDir } from 'can-write-to-dir'
import PATH from 'path-name'

export default function (
  knownCandidates: string[] = [],
  { shouldAllowWrite = true }: { shouldAllowWrite?: boolean } = {}
) {
  if (!process.env[PATH]) {
    throw new PnpmError('NO_PATH_ENV',
      `Couldn't find a global directory for executables because the "${PATH}" environment variable is not set.`)
  }
  const dirs = process.env[PATH]?.split(path.delimiter) ?? []
  const nodeBinDir = path.dirname(process.execPath)
  return pickBestGlobalBinDir(dirs, [
    ...knownCandidates,
    nodeBinDir,
  ], shouldAllowWrite)
}

const areDirsEqual = (dir1: string, dir2: string) =>
  path.relative(dir1, dir2) === ''

function pickBestGlobalBinDir (
  dirs: string[],
  knownCandidates: string[],
  shouldAllowWrite: boolean
) {
  const noWriteAccessDirs = [] as string[]
  const pnpmDir = dirs.find((dir) => isUnderDir('pnpm', dir.toLowerCase()))
  if (pnpmDir != null) {
    if (canWriteToDirAndExists(pnpmDir)) return pnpmDir
    throw new PnpmError('PNPM_DIR_NOT_WRITABLE', `The CLI has no write access to the pnpm home directory at ${pnpmDir}`)
  }
  for (const dir of dirs) {
    const lowCaseDir = dir.toLowerCase()
    if ((
      isUnderDir('node', lowCaseDir) ||
      isUnderDir('nodejs', lowCaseDir) ||
      isUnderDir('npm', lowCaseDir) ||
      knownCandidates.some((candidate) => areDirsEqual(candidate, dir)) ||
      dirHasNodeRelatedCommand(dir)
    ) && !isUnderDir('_npx', lowCaseDir)) {
      if (canWriteToDirAndExists(dir)) return dir
      noWriteAccessDirs.push(dir)
    }
  }
  if (noWriteAccessDirs.length === 0) {
    throw new PnpmError('NO_GLOBAL_BIN_DIR', "Couldn't find a suitable global executables directory.", {
      hint: `There should be a node, nodejs, npm, or pnpm directory in the "${PATH}" environment variable`,
    })
  }
  if (shouldAllowWrite) {
    throw new PnpmError('GLOBAL_BIN_DIR_PERMISSION', 'No write access to the found global executable directories', {
      hint: `The found directories:
  ${noWriteAccessDirs.join('\n')}`,
    })
  }
  return noWriteAccessDirs[0]
}

const NODE_RELATED_COMMANDS = new Set(['pnpm', 'npm', 'node'])

function dirHasNodeRelatedCommand (dir: string) {
  try {
    return fs.readdirSync(dir, { withFileTypes: true })
      // We are searching for files or symlinks, not directories
      .filter((entry) => !entry.isDirectory())
      .map(({ name }) => name.toLowerCase())
      .some((file) => NODE_RELATED_COMMANDS.has(file.split('.')[0]))
  } catch (err) {
    return false
  }
}

function isUnderDir (dir: string, target: string) {
  target = target.endsWith(path.sep) ? target : `${target}${path.sep}`
  return target.includes(`${path.sep}${dir}${path.sep}`) ||
    target.includes(`${path.sep}.${dir}${path.sep}`)
}

function canWriteToDirAndExists (dir: string) {
  try {
    return canWriteToDir(dir)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return false
  }
}
