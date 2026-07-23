import fs from 'node:fs'
import fsPromises from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { docsUrl } from '@pnpm/cli.utils'
import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { realpathMissing } from 'realpath-missing'
import { renameOverwriteSync } from 'rename-overwrite'
import { renderHelp } from 'render-help'
import { safeExeca as execa } from 'safe-execa'
import * as shlex from 'shlex'

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    editor: String,
  }
}

export const shorthands = {}

export const commandNames = ['edit']

export const recursiveByDefault = false

export function help (): string {
  return renderHelp({
    description: 'Open an installed package\'s folder in the default text editor.',
    descriptionLists: [{
      title: 'Options',
      list: [
        {
          description: 'The editor to use for opening the package',
          name: '--editor <editor>',
        },
      ],
    }],
    url: docsUrl('edit'),
    usages: ['pnpm edit <pkg>[/<subpkg>...]'],
  })
}

export type EditCommandOptions = Pick<Config, 'dir' | 'modulesDir'> & {
  editor?: string
}

/**
 * Canonicalize a PATH directory and check it is outside the project root.
 * Returns the canonical directory path if safe, or null if the directory
 * cannot be resolved or is inside the project tree.
 */
function isSafePathDir (dir: string, projectRoot: string): string | null {
  let realDir: string
  try {
    realDir = fs.realpathSync(dir)
  } catch {
    return null
  }
  const relative = path.relative(projectRoot, realDir)
  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    return realDir
  }
  return null
}

/**
 * Check that a candidate binary is not a symlink into the project root.
 */
function isSafeCandidate (candidate: string, projectRoot: string): boolean {
  try {
    const realCandidate = fs.realpathSync(candidate)
    const relative = path.relative(projectRoot, realCandidate)
    return relative.startsWith('..') || path.isAbsolute(relative)
  } catch {
    return false
  }
}

/**
 * Resolve a safe pnpm executable from PATH, excluding entries inside
 * the project root to prevent project-controlled directory hijacking.
 */
export function resolveSafePnpmPath (projectRoot: string): string {
  const envPath = process.env.PATH || ''
  const pathDirs = envPath.split(path.delimiter)
  const exts = process.platform === 'win32' ? ['.exe', '.cmd', '.bat', '.ps1', ''] : ['']

  for (const dir of pathDirs) {
    if (!dir || !path.isAbsolute(dir)) {
      continue
    }
    const safeDir = isSafePathDir(dir, projectRoot)
    if (safeDir == null) {
      continue
    }
    for (const ext of exts) {
      const candidate = path.join(dir, `pnpm${ext}`)
      try {
        const stat = fs.statSync(candidate)
        if (stat.isFile()) {
          if (!isSafeCandidate(candidate, projectRoot)) {
            continue
          }
          if (process.platform !== 'win32') {
            fs.accessSync(candidate, fs.constants.X_OK)
          }
          return candidate
        }
      } catch {
        // ignore
      }
    }
  }
  throw new PnpmError('EDIT_PNPM_NOT_FOUND', 'Could not find a safe pnpm executable on the PATH')
}

/**
 * Resolve a bare executable name to its first PATH match that is outside
 * the project root.  On Windows, `child_process.spawn` with a bare name
 * searches the current directory before PATH, so a repo-local file named
 * `notepad.exe` could hijack the editor command.
 */
function resolveSafeEditorPath (cmd: string, projectRoot: string): string | null {
  const hasSep = cmd.includes('/') || (process.platform === 'win32' && cmd.includes('\\'))
  if (hasSep) return null

  const envPath = process.env.PATH || ''
  const pathDirs = envPath.split(path.delimiter)
  const exts = process.platform === 'win32' ? ['.exe', '.cmd', '.bat', '.ps1', ''] : ['']

  for (const dir of pathDirs) {
    if (!dir || !path.isAbsolute(dir)) {
      continue
    }
    const safeDir = isSafePathDir(dir, projectRoot)
    if (safeDir == null) {
      continue
    }
    for (const ext of exts) {
      const candidate = path.join(dir, `${cmd}${ext}`)
      try {
        const stat = fs.statSync(candidate)
        if (stat.isFile()) {
          if (!isSafeCandidate(candidate, projectRoot)) {
            continue
          }
          if (process.platform !== 'win32') {
            fs.accessSync(candidate, fs.constants.X_OK)
          }
          return candidate
        }
      } catch {
        // ignore
      }
    }
  }
  return null
}

export async function handler (opts: EditCommandOptions, params: string[]): Promise<void> {
  if (!params[0]) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm edit` requires the package name')
  }

  const lockfileDir = await fsPromises.realpath(opts.dir ?? process.cwd())
  const pkgNameAndSubpkg = params[0]

  const segments = pkgNameAndSubpkg.split(/[/\\]/)
  for (const seg of segments) {
    if (!seg || seg === '.' || seg === '..' || seg.includes(':')) {
      throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package path segment: '${seg}'`)
    }
  }

  const parts: string[] = []
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i]
    if (seg.startsWith('@')) {
      if (i + 1 < segments.length) {
        parts.push(`${seg}/${segments[i + 1]}`)
        i++
      } else {
        throw new PnpmError('INVALID_PACKAGE_NAME', `Incomplete scoped package name: '${seg}'`)
      }
    } else {
      parts.push(seg)
    }
  }

  const expectedRoot = await realpathMissing(path.join(lockfileDir, opts.modulesDir ?? 'node_modules'))

  let currentDir = expectedRoot
  for (let i = 0; i < parts.length; i++) {
    const part = parts[i]
    const candidatePath = i === 0 ? path.join(currentDir, part) : path.join(currentDir, 'node_modules', part)
    try {
      await fsPromises.access(candidatePath) // eslint-disable-line no-await-in-loop
    } catch {
      throw new PnpmError(
        'EDIT_PACKAGE_NOT_FOUND',
        `Could not find package '${part}' under '${currentDir}'`
      )
    }
    const resolvedPath = await fsPromises.realpath(candidatePath) // eslint-disable-line no-await-in-loop
    const relative = path.relative(expectedRoot, resolvedPath)
    if (relative.startsWith('..') || path.isAbsolute(relative)) {
      throw new PnpmError(
        'EDIT_PACKAGE_OUT_OF_TREE',
        `Resolved path for package '${part}' lies outside the expected node_modules tree: '${resolvedPath}'`
      )
    }
    currentDir = resolvedPath
  }

  const realPkgPath = currentDir

  await deHardlinkDir(realPkgPath)

  const editor = opts.editor || process.env.EDITOR || process.env.VISUAL || (process.platform === 'win32' ? 'notepad' : 'vi')

  let editorParts: string[]
  try {
    editorParts = shlex.split(editor)
  } catch (err: unknown) {
    throw new PnpmError('INVALID_EDITOR', `Failed to parse editor command: ${err instanceof Error ? err.message : String(err)}`)
  }
  if (editorParts.length === 0) {
    throw new PnpmError('INVALID_EDITOR', 'No editor command found')
  }
  const cmd = editorParts[0]
  const args = [...editorParts.slice(1), realPkgPath]

  const safeCmd = resolveSafeEditorPath(cmd, lockfileDir)
  const finalCmd = safeCmd ?? cmd

  try {
    await execa(finalCmd, args, {
      stdio: 'inherit',
    })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'exitCode' in err && typeof err.exitCode === 'number') {
      throw new PnpmError('EDITOR_EXIT_ERROR', `Editor exited with code ${err.exitCode}`)
    }
    if (util.types.isNativeError(err) && 'signal' in err && err.signal) {
      throw new PnpmError('EDITOR_SIGNAL_ERROR', `Editor was terminated with signal ${err.signal}`)
    }
    const reason = err instanceof Error ? err.message : String(err)
    throw new PnpmError('EDITOR_SPAWN_ERROR', `Failed to launch editor '${cmd}': ${reason}`)
  }

  const pkgToRebuild = parts[parts.length - 1]
  const execPath = process.execPath
  let pnpmPath: string
  let rebuildArgs: string[]

  const monorepoBin = path.resolve(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
  if (fs.existsSync(monorepoBin)) {
    pnpmPath = execPath
    rebuildArgs = [monorepoBin, 'rebuild', pkgToRebuild]
  } else {
    const pnpmScript = process.argv[1]
    if (pnpmScript && (pnpmScript.endsWith('.js') || pnpmScript.endsWith('.cjs') || pnpmScript.endsWith('.mjs') || pnpmScript.endsWith('.ts')) && pnpmScript.includes('pnpm')) {
      pnpmPath = execPath
      rebuildArgs = [pnpmScript, 'rebuild', pkgToRebuild]
    } else {
      pnpmPath = resolveSafePnpmPath(lockfileDir)
      rebuildArgs = ['rebuild', pkgToRebuild]
    }
  }

  try {
    await execa(pnpmPath, rebuildArgs, {
      cwd: lockfileDir,
      stdio: 'inherit',
    })
  } catch (err) {
    throw new PnpmError('REBUILD_FAILURE', `Failed to rebuild package '${pkgToRebuild}' after editing: ${err instanceof Error ? err.message : String(err)}`)
  }
}

async function deHardlinkDir (dir: string): Promise<void> {
  const files = await fsPromises.readdir(dir)
  const tmpDir = fs.mkdtempSync(path.join(dir, `.pnpm-edit-${path.basename(dir)}-`))
  const renames: Array<[string, string]> = []
  const tasks: Array<() => Promise<void>> = []
  try {
    const entries = await Promise.all(
      files.map(async (file) => {
        const filePath = path.join(dir, file)
        const stat = await fsPromises.lstat(filePath)
        return { file, filePath, stat }
      })
    )
    const dirTasks: Array<Promise<void>> = []
    for (const { file, filePath, stat } of entries) {
      if (stat.isSymbolicLink()) {
        continue
      }
      if (stat.isDirectory()) {
        dirTasks.push(deHardlinkDir(filePath))
      } else if (stat.isFile()) {
        if (stat.nlink <= 1) {
          continue
        }
        const writableMode = stat.mode | 0o200
        const tmpFile = path.join(tmpDir, file)
        renames.push([tmpFile, filePath])
        tasks.push(async () => {
          await fsPromises.copyFile(filePath, tmpFile)
          await fsPromises.chmod(tmpFile, writableMode)
        })
      }
    }
    await Promise.all(dirTasks)
    await Promise.all(tasks.map(t => t()))
    for (const [tmpFile, filePath] of renames) {
      renameOverwriteSync(tmpFile, filePath)
    }
  } finally {
    try {
      await fsPromises.rmdir(tmpDir)
    } catch {
      // ignore
    }
  }
}
