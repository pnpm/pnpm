import path from 'node:path'

import { isSubdir, strict as isStrictSubdir } from 'is-subdir'

import {
  isCleanableModulesDir,
  type PlannedModulesTarget,
  removesEntryInModulesDir,
  resolveInside,
} from './cleanRemoval.js'

export function filterCleanableTargets (
  candidates: Array<{ ownerDir: string | undefined, resolved: string | undefined }>,
  opts: {
    preservedEntry: string | undefined
    protectedMemberDirs: string[]
    sharedModulesDir: string | undefined
  }
): PlannedModulesTarget[] {
  const seen = new Set<string>()
  const unique: PlannedModulesTarget[] = []
  for (const { ownerDir, resolved } of candidates) {
    if (isCleanableModulesDir(resolved, seen, { ownerDir, protectedMemberDirs: opts.protectedMemberDirs })) {
      seen.add(resolved)
      unique.push({
        dir: resolved,
        mode: resolved === opts.sharedModulesDir
          ? { kind: 'preserve-shared-virtual-store', preservedEntry: opts.preservedEntry }
          : { kind: 'all' },
      })
    }
  }
  return unique
}

export async function collectLockfileTargets (
  scope: {
    cleansRoot: boolean
    dirs: string[]
    realRootDir: string
    resolvedLockfileDir: string | undefined
    rootDir: string
  }
): Promise<string[]> {
  const candidateDirs = [...scope.dirs]
  if (scope.cleansRoot) {
    candidateDirs.push(scope.rootDir)
    if (scope.resolvedLockfileDir != null) {
      candidateDirs.push(scope.resolvedLockfileDir)
    }
  }
  const resolvedPaths = await Promise.all(candidateDirs.map(async (dir) => {
    const lockfilePath = await resolveInside(dir, 'pnpm-lock.yaml')
    return lockfilePath != null && isStrictSubdir(scope.realRootDir, lockfilePath) ? lockfilePath : undefined
  }))
  return Array.from(new Set(resolvedPaths.filter((item): item is string => item != null)))
}

export async function planRemovesInstallState (
  opts: {
    externalVirtualStoreDir: string | undefined
    installStateDir: string
    modulesTargets: PlannedModulesTarget[]
    realRootDir: string
  }
): Promise<boolean> {
  if (await removesViaAncestor(opts.realRootDir, opts.installStateDir, opts.modulesTargets)) {
    return true
  }
  const realStateDir = await resolveInside(opts.realRootDir, opts.installStateDir)
  if (realStateDir == null) return false
  if (opts.externalVirtualStoreDir != null && isSubdir(opts.externalVirtualStoreDir, realStateDir)) {
    return true
  }
  return removesSubpathInTargets(opts.modulesTargets, realStateDir)
}

async function removesSubpathInTargets (
  modulesTargets: PlannedModulesTarget[],
  realStateDir: string
): Promise<boolean> {
  const checks = await Promise.all(modulesTargets.map(async ({ dir, mode }) => {
    if (!isSubdir(dir, realStateDir)) return false
    const rel = path.relative(dir, realStateDir)
    const entryName = rel === '' ? 'lock.yaml' : rel.split(path.sep)[0]
    return removesEntryInModulesDir(dir, mode, entryName)
  }))
  return checks.includes(true)
}

async function removesViaAncestor (
  realRootDir: string,
  installStateDir: string,
  modulesTargets: PlannedModulesTarget[]
): Promise<boolean> {
  const steps = collectAncestorSteps(installStateDir)
  const results = await Promise.all(steps.map(async ({ childName, parentDir }) => {
    const resolvedParent = await resolveInside(realRootDir, parentDir)
    if (resolvedParent == null) return false
    return targetRemovesChild(modulesTargets, resolvedParent, childName)
  }))
  return results.includes(true)
}

async function targetRemovesChild (
  modulesTargets: PlannedModulesTarget[],
  resolvedParent: string,
  childName: string
): Promise<boolean> {
  const checks = await Promise.all(modulesTargets.map(async ({ dir, mode }) => (
    dir === resolvedParent ? removesEntryInModulesDir(dir, mode, childName) : false
  )))
  return checks.includes(true)
}

function collectAncestorSteps (targetPath: string): Array<{ childName: string, parentDir: string }> {
  const steps: Array<{ childName: string, parentDir: string }> = []
  let current = path.resolve(targetPath)
  while (true) {
    const parsed = path.parse(current)
    if (!parsed.base || parsed.dir === current) return steps
    steps.push({ childName: parsed.base, parentDir: parsed.dir })
    current = parsed.dir
  }
}
