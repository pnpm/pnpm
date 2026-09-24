import fs from 'node:fs'
import path from 'node:path'

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
   * The patch file path as Yarn recorded it. A `~/` prefix is relative to the
   * Yarn project root, any other path to the declaring project.
   */
  patchPath: string
}

export function parseYarnPatchSpecifier (specifier: string): YarnPatchSpecifier | undefined {
  if (!specifier.startsWith('patch:')) return undefined
  const { source, selector } = structUtils.parseRange(specifier)
  if (source == null) return undefined
  const [name, range] = splitDescriptor(source) ?? []
  if (name == null || range == null) return undefined
  if (!range.startsWith('npm:')) {
    return { specifier: range, patchKey: getPatchKey(name, range), patchPath: selector }
  }
  const npmRange = range.slice('npm:'.length)
  const [target, targetRange] = splitDescriptor(npmRange) ?? []
  if (target != null && targetRange != null) {
    return { specifier: range, patchKey: getPatchKey(target, targetRange), patchPath: selector }
  }
  return { specifier: npmRange, patchKey: getPatchKey(name, npmRange), patchPath: selector }
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
 * Replaces every `patch:` specifier in the projects' manifests with the
 * specifier of the package it patches, and records each patch file that exists
 * in the `patchedDependencies` of pnpm-workspace.yaml. An entry already in
 * `patchedDependencies` is kept.
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
  for (const { project, patches } of patchedProjects) {
    for (const [alias, patch] of patches) {
      const patchFile = patch.patchPath.startsWith('~/')
        ? path.join(opts.yarnRootDir, patch.patchPath.slice(2))
        : path.resolve(project.rootDir, patch.patchPath)
      if (!fs.statSync(patchFile, { throwIfNoEntry: false })?.isFile()) {
        globalWarn(`The patch file ${patchFile} of "${alias}" does not exist. "${alias}" was imported without the patch.`)
        continue
      }
      if (opts.patchedDependencies?.[patch.patchKey] == null) {
        recorded[patch.patchKey] ??= patchFile
      }
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
    Object.entries(patchedDependencies).map(([key, patchFile]) => [
      key,
      path.isAbsolute(patchFile) ? normalizePath(path.relative(workspaceDir, patchFile)) : patchFile,
    ])
  )
}
