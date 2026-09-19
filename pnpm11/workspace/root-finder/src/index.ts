import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { isWorkspaceProjectDir } from '@pnpm/workspace.package-patterns'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import * as find from 'empathic/find'

const MANIFEST_BASE_NAMES_SET = new Set<string>(MANIFEST_BASE_NAMES)
const WORKSPACE_DIR_ENV_VAR = 'NPM_CONFIG_WORKSPACE_DIR'
const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'
const INVALID_WORKSPACE_MANIFEST_FILENAME = [
  'pnpm-workspaces.yaml',
  'pnpm-workspaces.yml',
  'pnpm-workspace.yml',
  '.pnpm-workspace.yaml',
  '.pnpm-workspace.yml',
  '.pnpm-workspaces.yaml',
  '.pnpm-workspaces.yml',
]

export async function findWorkspaceDir (cwd: string): Promise<string | undefined> {
  const workspaceManifestDirEnvVar = process.env[WORKSPACE_DIR_ENV_VAR] ?? process.env[WORKSPACE_DIR_ENV_VAR.toLowerCase()]
  if (workspaceManifestDirEnvVar) {
    return path.dirname(path.join(workspaceManifestDirEnvVar, WORKSPACE_MANIFEST_FILENAME))
  }
  const realCwd = await getRealPath(cwd)
  const workspaceManifestLocation = find.any([WORKSPACE_MANIFEST_FILENAME, ...INVALID_WORKSPACE_MANIFEST_FILENAME], { cwd: realCwd })
  if (!workspaceManifestLocation) return undefined
  if (path.basename(workspaceManifestLocation) !== WORKSPACE_MANIFEST_FILENAME) {
    throw new PnpmError('BAD_WORKSPACE_MANIFEST_NAME', `The workspace manifest file should be named "pnpm-workspace.yaml". File found: ${workspaceManifestLocation}`)
  }
  const workspaceDir = path.dirname(workspaceManifestLocation)
  return await belongsToWorkspace(workspaceDir, realCwd) ? workspaceDir : undefined
}

/**
 * A project that the workspace does not include stands on its own, so pnpm
 * runs it as a standalone project instead of acting on the whole workspace
 * (https://github.com/pnpm/pnpm/issues/3561).
 *
 * A directory without a manifest of its own is not such a project. It is some
 * place inside the workspace, like a package's source directory, and a command
 * run from there still acts on the workspace.
 */
async function belongsToWorkspace (workspaceDir: string, dir: string): Promise<boolean> {
  if (path.relative(workspaceDir, dir) === '' || !await hasProjectManifest(dir)) return true
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)
  return isWorkspaceProjectDir({ workspaceDir, dir, patterns: workspaceManifest?.packages ?? ['.'] })
}

async function hasProjectManifest (dir: string): Promise<boolean> {
  let entries: string[]
  try {
    entries = await fs.promises.readdir(dir)
  } catch (err: unknown) {
    // The directory pnpm was pointed at may not exist. Reporting that is the
    // job of the command that needs it, not of the workspace lookup.
    if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return false
    throw err
  }
  return entries.some((entry) => MANIFEST_BASE_NAMES_SET.has(entry))
}

async function getRealPath (path: string): Promise<string> {
  return new Promise<string>((resolve) => {
    // We need to resolve the real native path for case-insensitive file systems.
    // For example, we can access file as C:\Code\Project as well as c:\code\projects
    // Without this we can face a problem when try to install packages with -w flag,
    // when root dir is using c:\code\projects but packages were found by C:\Code\Project
    fs.realpath.native(path, function (err, resolvedPath) {
      resolve(err !== null ? path : resolvedPath)
    })
  })
}
