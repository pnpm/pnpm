import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { isEmptyDirOrNothing } from '@pnpm/fs.is-empty-dir-or-nothing'
import { logger } from '@pnpm/logger'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir } from 'is-subdir'

/**
 * Leaves an empty deploy directory in place, deleting existing content only
 * with `force`. A target inside the workspace is created one component at a
 * time, refusing any component that is a symlink or not a directory.
 */
export async function prepareDeployDir (
  deployDir: string,
  opts: {
    force?: boolean
    workspaceDir: string
  }
): Promise<void> {
  const normalizedDeployDir = path.resolve(deployDir)
  const normalizedWorkspaceDir = path.resolve(opts.workspaceDir)
  const workspaceChildTarget = isChildPath(normalizedDeployDir, normalizedWorkspaceDir)
  if (workspaceChildTarget) {
    createWorkspaceChildTargetParents(normalizedWorkspaceDir, normalizedDeployDir)
  }

  if (!isEmptyDirOrNothing(deployDir)) {
    if (!opts.force) {
      throw new PnpmError('DEPLOY_DIR_NOT_EMPTY', `Deploy path ${deployDir} is not empty`)
    }

    logger.warn({ message: 'using --force, deleting deploy path', prefix: deployDir })
  }

  if (workspaceChildTarget) {
    validateWorkspaceChildTargetComponents(normalizedWorkspaceDir, normalizedDeployDir)
  }
  await rimraf(deployDir)
  if (workspaceChildTarget) {
    createWorkspaceChildTargetParents(normalizedWorkspaceDir, normalizedDeployDir)
    createWorkspaceChildTargetDir(normalizedWorkspaceDir, normalizedDeployDir)
  } else {
    await fs.promises.mkdir(deployDir, { recursive: true })
  }
}

export function validateDeployTarget (
  deployDir: string,
  opts: {
    dir: string
    force?: boolean
    projectDir: string
    workspaceDir: string
  }
): void {
  const normalizedDeployDir = path.resolve(deployDir)
  const workspaceDir = path.resolve(opts.workspaceDir)
  const projectDir = path.resolve(opts.projectDir)
  const dir = path.resolve(opts.dir)
  if (samePath(normalizedDeployDir, workspaceDir)) {
    throw unsafeDeployTarget(normalizedDeployDir, 'target is the workspace root')
  }
  if (isAncestorPath(normalizedDeployDir, workspaceDir)) {
    throw unsafeDeployTarget(normalizedDeployDir, 'target contains the workspace root')
  }
  if (samePath(normalizedDeployDir, projectDir)) {
    throw unsafeDeployTarget(normalizedDeployDir, 'target is the selected project root')
  }
  if (isAncestorPath(normalizedDeployDir, projectDir)) {
    throw unsafeDeployTarget(normalizedDeployDir, 'target contains the selected project')
  }
  if (samePath(normalizedDeployDir, dir)) {
    throw unsafeDeployTarget(normalizedDeployDir, 'target is the current directory')
  }
  if (isAncestorPath(normalizedDeployDir, dir)) {
    throw unsafeDeployTarget(normalizedDeployDir, 'target contains the current directory')
  }
  if (opts.force && !isChildPath(normalizedDeployDir, workspaceDir)) {
    throw unsafeDeployTarget(normalizedDeployDir, 'target is outside the workspace')
  }
  if (isChildPath(normalizedDeployDir, workspaceDir)) {
    validateWorkspaceChildTargetComponents(workspaceDir, normalizedDeployDir)
  }
}

function validateWorkspaceChildTargetComponents (workspaceDir: string, deployDir: string): void {
  const relative = path.relative(workspaceDir, deployDir)
  let current = workspaceDir
  for (const component of relative.split(path.sep)) {
    if (!component) continue
    current = path.join(current, component)
    let stat: fs.Stats
    try {
      stat = fs.lstatSync(current)
    } catch (error: unknown) {
      if (isENOENT(error)) return
      throw error
    }
    if (stat.isSymbolicLink()) {
      throw unsafeDeployTarget(current, 'target path contains a symlink')
    }
  }
}

function createWorkspaceChildTargetParents (workspaceDir: string, deployDir: string): void {
  const parent = path.dirname(deployDir)
  const relative = path.relative(workspaceDir, parent)
  let current = workspaceDir
  for (const component of relative.split(path.sep)) {
    if (!component) continue
    current = path.join(current, component)
    createWorkspaceChildTargetComponent(current)
  }
}

function createWorkspaceChildTargetDir (workspaceDir: string, deployDir: string): void {
  try {
    fs.mkdirSync(deployDir)
  } catch (error: unknown) {
    if (isEEXIST(error)) {
      throw unsafeDeployTarget(deployDir, 'target changed during deploy preparation')
    }
    throw error
  }
  validateWorkspaceChildTargetComponents(workspaceDir, deployDir)
}

function createWorkspaceChildTargetComponent (component: string): void {
  try {
    fs.mkdirSync(component)
  } catch (error: unknown) {
    if (!isEEXIST(error)) throw error
  }
  const stat = fs.lstatSync(component)
  if (stat.isSymbolicLink()) {
    throw unsafeDeployTarget(component, 'target path contains a symlink')
  }
  if (!stat.isDirectory()) {
    throw unsafeDeployTarget(component, 'target path contains a non-directory')
  }
}

function unsafeDeployTarget (deployDir: string, reason: string): PnpmError {
  return new PnpmError('INVALID_DEPLOY_TARGET', `Refusing to deploy to unsafe target ${deployDir}: ${reason}`)
}

function samePath (left: string, right: string): boolean {
  return path.normalize(left) === path.normalize(right)
}

function isAncestorPath (parent: string, child: string): boolean {
  return isChildPath(child, parent)
}

function isChildPath (child: string, parent: string): boolean {
  return !samePath(child, parent) && isSubdir(parent, child)
}

function isENOENT (error: unknown): boolean {
  return typeof error === 'object' && error != null && 'code' in error && error.code === 'ENOENT'
}

function isEEXIST (error: unknown): boolean {
  return typeof error === 'object' && error != null && 'code' in error && error.code === 'EEXIST'
}
