import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { writeSettings } from '@pnpm/config.writer'
import { globalWarn } from '@pnpm/logger'
import type { ProjectManifest } from '@pnpm/types'
import { semverUtils, structUtils } from '@yarnpkg/core'
import normalizePath from 'normalize-path'

const DEPENDENCY_FIELDS = ['dependencies', 'devDependencies', 'optionalDependencies'] as const

/**
 * A dependency specifier written by Yarn's `patch:` protocol, such as
 * `patch:foo@npm%3A1.2.3#~/.yarn/patches/foo.patch`.
 */
export interface YarnPatchSpecifier {
  /** The specifier of the package the patch applies to, in the form pnpm resolves. */
  specifier: string
  /** The `patchedDependencies` key selecting the patched package. */
  patchKey: string
  /**
   * The patch file paths as Yarn recorded them, without Yarn's builtin
   * compatibility patches. A `~/` prefix is relative to the Yarn project root,
   * any other path to the declaring project.
   */
  patchPaths: string[]
}

export function parseYarnPatchSpecifier (specifier: string): YarnPatchSpecifier | undefined {
  if (!specifier.startsWith('patch:')) return undefined
  const { source, selector } = structUtils.parseRange(specifier)
  if (source == null) return undefined
  const [name, range] = splitDescriptor(source) ?? []
  if (name == null || range == null) return undefined
  const patchPaths = selector.split('&').filter((patchPath) => !isBuiltinPatch(patchPath))
  if (!range.startsWith('npm:')) {
    return { specifier: range, patchKey: getPatchKey(name, range), patchPaths }
  }
  const npmRange = range.slice('npm:'.length)
  const [target, targetRange] = splitDescriptor(npmRange) ?? []
  if (target != null && targetRange != null) {
    return { specifier: range, patchKey: getPatchKey(target, targetRange), patchPaths }
  }
  return { specifier: npmRange, patchKey: getPatchKey(name, npmRange), patchPaths }
}

// Yarn's builtin patches, such as `optional!builtin<compat/typescript>`, adapt
// packages to Plug'n'Play and have no file.
function isBuiltinPatch (patchPath: string): boolean {
  return patchPath.replace(/^optional!/, '').startsWith('builtin<')
}

function splitDescriptor (descriptor: string): [string, string] | undefined {
  const at = descriptor.indexOf('@', 1)
  if (at === -1) return undefined
  return [descriptor.slice(0, at), descriptor.slice(at + 1)]
}

function getPatchKey (name: string, range: string): string {
  return semverUtils.validRange(range) == null ? name : `${name}@${range}`
}

export interface ImportedProject {
  rootDir: string
  manifest: ProjectManifest
  writeProjectManifest: (manifest: ProjectManifest) => Promise<void>
}

export interface ImportYarnPatchesOptions {
  projects: ImportedProject[]
  yarnRootDir: string
  workspaceDir: string
  patchedDependencies?: Record<string, string>
}

/**
 * An entry already in `patchedDependencies`, or recorded earlier, wins over a
 * converted patch with the same key.
 *
 * @returns the `patchedDependencies` to import with, with absolute patch file
 * paths, or `undefined` when no patch was recorded.
 */
export async function importYarnPatches (opts: ImportYarnPatchesOptions): Promise<Record<string, string> | undefined> {
  const patchedProjects = opts.projects
    .map((project) => ({ project, patches: replacePatchSpecifiers(project.manifest) }))
    .filter(({ patches }) => patches.length > 0)
  await Promise.all(patchedProjects.map(({ project }) => project.writeProjectManifest(project.manifest)))
  const recorded: Record<string, string> = {}
  const patchFiles = patchedProjects.flatMap(({ project, patches }) => patches.map(([alias, patch]) => ({
    alias,
    patch,
    patchFile: patch.patchPaths.length === 1 ? resolvePatchFile(patch.patchPaths[0], opts.yarnRootDir, project.rootDir) : undefined,
  })))
  const patchFileExists = await Promise.all(patchFiles.map(({ patchFile }) => patchFile != null && isFile(patchFile)))
  for (const [index, { alias, patch, patchFile }] of patchFiles.entries()) {
    if (patch.patchPaths.length > 1) {
      globalWarn(`"${alias}" has several Yarn patches, and pnpm applies one patch per dependency. "${alias}" was imported without the patches.`)
      continue
    }
    if (patchFile == null) continue
    if (!patchFileExists[index]) {
      globalWarn(`The patch file ${patchFile} of "${alias}" does not exist. "${alias}" was imported without the patch.`)
      continue
    }
    const kept = opts.patchedDependencies?.[patch.patchKey] ?? recorded[patch.patchKey]
    if (kept == null) {
      recorded[patch.patchKey] = patchFile
    } else if (path.resolve(opts.workspaceDir, kept) !== patchFile) {
      globalWarn(`The Yarn patch ${toWorkspacePath(patchFile, opts.workspaceDir)} of "${alias}" was not applied, because "${patch.patchKey}" already uses the patch ${toWorkspacePath(kept, opts.workspaceDir)}.`)
    }
  }
  if (Object.keys(recorded).length === 0) return undefined
  const patchedDependencies = { ...opts.patchedDependencies, ...recorded }
  await writeSettings({
    rootProjectManifestDir: opts.workspaceDir,
    workspaceDir: opts.workspaceDir,
    updatedSettings: {
      patchedDependencies: relativizePatchFiles(patchedDependencies, opts.workspaceDir),
    },
  })
  return Object.fromEntries(
    Object.entries(patchedDependencies).map(([key, patchFile]) => [key, path.resolve(opts.workspaceDir, patchFile)])
  )
}

function resolvePatchFile (patchPath: string, yarnRootDir: string, projectDir: string): string {
  return patchPath.startsWith('~/') ? path.join(yarnRootDir, patchPath.slice(2)) : path.resolve(projectDir, patchPath)
}

async function isFile (filePath: string): Promise<boolean> {
  try {
    return (await fs.promises.stat(filePath)).isFile()
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

function replacePatchSpecifiers (manifest: ProjectManifest): Array<[string, YarnPatchSpecifier]> {
  const patches: Array<[string, YarnPatchSpecifier]> = []
  for (const field of DEPENDENCY_FIELDS) {
    const dependencies = manifest[field]
    if (dependencies == null) continue
    for (const [alias, specifier] of Object.entries(dependencies)) {
      const patch = parseYarnPatchSpecifier(specifier)
      if (patch == null) continue
      dependencies[alias] = patch.specifier
      patches.push([alias, patch])
    }
  }
  return patches
}

function relativizePatchFiles (patchedDependencies: Record<string, string>, workspaceDir: string): Record<string, string> {
  return Object.fromEntries(
    Object.entries(patchedDependencies).map(([key, patchFile]) => [key, toWorkspacePath(patchFile, workspaceDir)])
  )
}

function toWorkspacePath (patchFile: string, workspaceDir: string): string {
  return path.isAbsolute(patchFile) ? normalizePath(path.relative(workspaceDir, patchFile)) : patchFile
}
