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

function isSafeCandidate (candidate: string, projectRoot: string): boolean {
  try {
    const realCandidate = fs.realpathSync(candidate)
    const relative = path.relative(projectRoot, realCandidate)
    return relative.startsWith('..') || path.isAbsolute(relative)
  } catch {
    return false
  }
}

function findExecutableInDir (dir: string, binaryName: string, projectRoot: string): string | null {
  const extensions = process.platform === 'win32' ? ['.exe', '.cmd', '.bat', '.ps1', ''] : ['']
  for (const extension of extensions) {
    const candidate = path.join(dir, `${binaryName}${extension}`)
    try {
      const stat = fs.statSync(candidate)
      if (!stat.isFile() || !isSafeCandidate(candidate, projectRoot)) {
        continue
      }
      if (process.platform !== 'win32') {
        fs.accessSync(candidate, fs.constants.X_OK)
      }
      return candidate
    } catch {
      // ignore inaccessible or non-executable candidates
    }
  }
  return null
}

function findSafeExecutable (binaryName: string, projectRoot: string): string | null {
  const envPath = process.env.PATH || ''
  const pathDirs = envPath.split(path.delimiter)
  for (const dir of pathDirs) {
    if (!dir || !path.isAbsolute(dir)) {
      continue
    }
    const safeDir = isSafePathDir(dir, projectRoot)
    if (safeDir == null) {
      continue
    }
    const executable = findExecutableInDir(safeDir, binaryName, projectRoot)
    if (executable != null) {
      return executable
    }
  }
  return null
}

export function resolveSafePnpmPath (projectRoot: string): string {
  const safePnpm = findSafeExecutable('pnpm', projectRoot)
  if (safePnpm != null) {
    return safePnpm
  }
  throw new PnpmError('EDIT_PNPM_NOT_FOUND', 'Could not find a safe pnpm executable on the PATH')
}

function resolveSafeEditorPath (commandName: string, projectRoot: string): string | null {
  const hasSeparator = commandName.includes('/') || (process.platform === 'win32' && commandName.includes('\\'))
  if (hasSeparator) return null
  return findSafeExecutable(commandName, projectRoot)
}

function validatePathSegments (segments: string[]): void {
  for (const segment of segments) {
    if (!segment || segment === '.' || segment === '..' || segment.includes(':')) {
      throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package path segment: '${segment}'`)
    }
  }
}

function parsePackagePath (pkgNameAndSubpkg: string): string[] {
  const segments = pkgNameAndSubpkg.split(/[/\\]/)
  validatePathSegments(segments)
  const parts: string[] = []
  let segmentIndex = 0
  while (segmentIndex < segments.length) {
    const segment = segments[segmentIndex]
    if (segment.startsWith('@')) {
      if (segmentIndex + 1 >= segments.length) {
        throw new PnpmError('INVALID_PACKAGE_NAME', `Incomplete scoped package name: '${segment}'`)
      }
      parts.push(`${segment}/${segments[segmentIndex + 1]}`)
      segmentIndex += 2
    } else {
      parts.push(segment)
      segmentIndex += 1
    }
  }
  return parts
}

async function resolvePackageDirectory (parts: string[], expectedRoot: string): Promise<string> {
  let currentDir = expectedRoot
  for (let partIndex = 0; partIndex < parts.length; partIndex++) {
    const part = parts[partIndex]
    const candidatePath = partIndex === 0 ? path.join(currentDir, part) : path.join(currentDir, 'node_modules', part)
    try {
      await fsPromises.access(candidatePath) // eslint-disable-line no-await-in-loop -- sequential tree descent
    } catch {
      throw new PnpmError('EDIT_PACKAGE_NOT_FOUND', `Could not find package '${part}' under '${currentDir}'`)
    }
    const resolvedPath = await fsPromises.realpath(candidatePath) // eslint-disable-line no-await-in-loop -- sequential tree descent
    const relative = path.relative(expectedRoot, resolvedPath)
    if (relative.startsWith('..') || path.isAbsolute(relative)) {
      throw new PnpmError('EDIT_PACKAGE_OUT_OF_TREE', `Resolved path for package '${part}' lies outside the expected node_modules tree: '${resolvedPath}'`)
    }
    currentDir = resolvedPath
  }
  return currentDir
}

function parseEditorCommand (editor: string): string[] {
  try {
    const editorParts = shlex.split(editor)
    if (editorParts.length === 0) {
      throw new PnpmError('INVALID_EDITOR', 'No editor command found')
    }
    return editorParts
  } catch (err: unknown) {
    if (err instanceof PnpmError) throw err
    throw new PnpmError('INVALID_EDITOR', `Failed to parse editor command: ${err instanceof Error ? err.message : String(err)}`)
  }
}

function handleEditorError (err: unknown, commandName: string): never {
  if (util.types.isNativeError(err) && 'exitCode' in err && typeof err.exitCode === 'number') {
    throw new PnpmError('EDITOR_EXIT_ERROR', `Editor exited with code ${err.exitCode}`)
  }
  if (util.types.isNativeError(err) && 'signal' in err && err.signal) {
    throw new PnpmError('EDITOR_SIGNAL_ERROR', `Editor was terminated with signal ${err.signal}`)
  }
  const reason = err instanceof Error ? err.message : String(err)
  throw new PnpmError('EDITOR_SPAWN_ERROR', `Failed to launch editor '${commandName}': ${reason}`)
}

async function launchEditor (editor: string, realPkgPath: string, lockfileDir: string): Promise<void> {
  const editorParts = parseEditorCommand(editor)
  const commandName = editorParts[0]
  const commandArgs = [...editorParts.slice(1), realPkgPath]
  const safeCommand = resolveSafeEditorPath(commandName, lockfileDir) ?? commandName

  try {
    await execa(safeCommand, commandArgs, { stdio: 'inherit' })
  } catch (err: unknown) {
    handleEditorError(err, commandName)
  }
}

function determinePnpmExecution (pkgToRebuild: string, lockfileDir: string): { pnpmPath: string, rebuildArgs: string[] } {
  const monorepoBin = path.resolve(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
  if (fs.existsSync(monorepoBin)) {
    return { pnpmPath: process.execPath, rebuildArgs: [monorepoBin, 'rebuild', pkgToRebuild] }
  }
  const pnpmScript = process.argv[1]
  if (pnpmScript && /\.[cm]?[jt]s$/.test(pnpmScript) && pnpmScript.includes('pnpm')) {
    return { pnpmPath: process.execPath, rebuildArgs: [pnpmScript, 'rebuild', pkgToRebuild] }
  }
  return { pnpmPath: resolveSafePnpmPath(lockfileDir), rebuildArgs: ['rebuild', pkgToRebuild] }
}

async function rebuildPackage (pkgToRebuild: string, lockfileDir: string): Promise<void> {
  const { pnpmPath, rebuildArgs } = determinePnpmExecution(pkgToRebuild, lockfileDir)
  try {
    await execa(pnpmPath, rebuildArgs, { cwd: lockfileDir, stdio: 'inherit' })
  } catch (err) {
    throw new PnpmError('REBUILD_FAILURE', `Failed to rebuild package '${pkgToRebuild}' after editing: ${err instanceof Error ? err.message : String(err)}`)
  }
}

export async function handler (opts: EditCommandOptions, params: string[]): Promise<void> {
  if (!params[0]) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm edit` requires the package name')
  }
  const lockfileDir = await fsPromises.realpath(opts.dir ?? process.cwd())
  const parts = parsePackagePath(params[0])
  const expectedRoot = await realpathMissing(path.join(lockfileDir, opts.modulesDir ?? 'node_modules'))
  const realPkgPath = await resolvePackageDirectory(parts, expectedRoot)

  await deHardlinkDir(realPkgPath)

  const defaultEditor = process.platform === 'win32' ? 'notepad' : 'vi'
  const chosenEditor = opts.editor || process.env.EDITOR || process.env.VISUAL || defaultEditor
  await launchEditor(chosenEditor, realPkgPath, lockfileDir)

  const pkgToRebuild = parts[parts.length - 1]
  await rebuildPackage(pkgToRebuild, lockfileDir)
}

async function processHardlinkEntry (
  entry: { file: string, filePath: string, stat: fs.Stats },
  tmpDir: string,
  renames: Array<[string, string]>,
  tasks: Array<() => Promise<void>>
): Promise<void> {
  const { file, filePath, stat } = entry
  if (stat.isSymbolicLink()) return
  if (stat.isDirectory()) {
    await deHardlinkDir(filePath)
  } else if (stat.isFile() && stat.nlink > 1) {
    const writableMode = stat.mode | 0o200
    const tmpFile = path.join(tmpDir, file)
    renames.push([tmpFile, filePath])
    tasks.push(async () => {
      await fsPromises.copyFile(filePath, tmpFile)
      await fsPromises.chmod(tmpFile, writableMode)
    })
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
    for (const entry of entries) {
      await processHardlinkEntry(entry, tmpDir, renames, tasks) // eslint-disable-line no-await-in-loop -- sequential processing of directories
    }
    await Promise.all(tasks.map(async (task) => task()))
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
