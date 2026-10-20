import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { parseWantedDependency, type ParseWantedDependencyResult } from '@pnpm/resolving.parse-wanted-dependency'
import type { PackageManifest } from '@pnpm/types'
import normalizePath from 'normalize-path'
import semver from 'semver'

import { type EditDirState, readEditDirState, readStateFile, type State } from './stateFile.js'

export interface ResolvePatchDirOptions {
  dir: string
  lockfileDir: string
  modulesDir: string
  extraModulesDirs?: string[]
}

export interface ResolvedPatchDir {
  editDir: string
  stateValue: EditDirState
}

export async function resolvePatchDir (
  userParam: string,
  opts: ResolvePatchDirOptions
): Promise<ResolvedPatchDir> {
  const editDirAtPath = findEditDirAtPath(userParam, opts)
  if (editDirAtPath) return editDirAtPath

  const matchInModules = await findEditDirInModulesDirs(userParam, [opts.modulesDir, ...(opts.extraModulesDirs ?? [])])
  if (matchInModules) return matchInModules

  throw invalidPatchDirError(userParam)
}

async function findEditDirInModulesDirs (
  userParam: string,
  modulesDirs: string[]
): Promise<ResolvedPatchDir | undefined> {
  const uniqueModulesDirs = Array.from(new Set(modulesDirs.map((dir) => path.resolve(dir))))
  const parsedDep = parseWantedDependency(userParam)
  const normalizedUserParam = normalizePath(userParam)
  const results = await Promise.all(
    uniqueModulesDirs.map(async (modulesDir) => collectModuleMatches(modulesDir, {
      userParam,
      normalizedUserParam,
      parsedDep,
      patchesDir: path.join(modulesDir, '.pnpm_patches'),
    }))
  )
  const exactMatches = results.flatMap((r) => r.exactMatches)
  const nameMatches = results.flatMap((r) => r.nameMatches)
  return pickSingleMatch(exactMatches, userParam) ?? pickSingleMatch(nameMatches, userParam)
}

async function collectModuleMatches (
  modulesDir: string,
  query: PatchDirQuery
): Promise<{ exactMatches: ResolvedPatchDir[], nameMatches: ResolvedPatchDir[] }> {
  const state = readStateFile(modulesDir)
  if (!state || Object.keys(state).length === 0) {
    return { exactMatches: [], nameMatches: [] }
  }
  const candidates = await readEditDirCandidates(state)
  const exactMatches: ResolvedPatchDir[] = []
  const nameMatches: ResolvedPatchDir[] = []
  for (const candidate of candidates) {
    if (!candidate) continue
    const matchKind = matchEditDirCandidate(candidate, query)
    if (matchKind === 'exact') {
      exactMatches.push({ editDir: candidate.candidateEditDir, stateValue: candidate.stateValue })
    } else if (matchKind === 'name') {
      nameMatches.push({ editDir: candidate.candidateEditDir, stateValue: candidate.stateValue })
    }
  }
  return { exactMatches, nameMatches }
}

/** The edit directory `userParam` names as a path, relative to `dir` or else to `lockfileDir`. */
function findEditDirAtPath (userParam: string, opts: ResolvePatchDirOptions): ResolvedPatchDir | undefined {
  const directPath = path.resolve(opts.dir, userParam)
  const editDirFromDir = readEditDirAt(directPath, opts)
  if (editDirFromDir) return editDirFromDir

  const directPathFromLockfileDir = path.resolve(opts.lockfileDir, userParam)
  if (directPathFromLockfileDir === directPath) return undefined
  return readEditDirAt(directPathFromLockfileDir, opts)
}

function readEditDirAt (editDir: string, opts: ResolvePatchDirOptions): ResolvedPatchDir | undefined {
  if (!isDirectory(editDir)) return undefined
  for (const modulesDir of candidateModulesDirsForEditDir(editDir, opts)) {
    const stateValue = readEditDirState({ editDir, modulesDir })
    if (stateValue) return { editDir, stateValue }
  }
  return undefined
}

function candidateModulesDirsForEditDir (editDir: string, opts: ResolvePatchDirOptions): string[] {
  const dirs = [opts.modulesDir]
  if (path.basename(path.dirname(editDir)) === '.pnpm_patches') {
    dirs.push(path.dirname(path.dirname(editDir)))
  }
  if (opts.extraModulesDirs) {
    dirs.push(...opts.extraModulesDirs)
  }
  return dirs
}

function isDirectory (dirPath: string): boolean {
  return fs.existsSync(dirPath) && fs.statSync(dirPath).isDirectory()
}

interface EditDirCandidate {
  candidateEditDir: string
  stateValue: EditDirState
  manifest: PackageManifest
}

async function readEditDirCandidates (state: State): Promise<Array<EditDirCandidate | undefined>> {
  return Promise.all(
    Object.entries(state).map(async ([candidateEditDir, stateValue]) => {
      if (!isDirectory(candidateEditDir)) {
        return undefined
      }

      try {
        const manifest = await readPackageJsonFromDir(candidateEditDir)
        return { candidateEditDir, stateValue, manifest }
      } catch {
        return undefined
      }
    })
  )
}

interface PatchDirQuery {
  userParam: string
  normalizedUserParam: string
  parsedDep: ParseWantedDependencyResult
  patchesDir: string
}

function matchEditDirCandidate (candidate: EditDirCandidate, query: PatchDirQuery): 'exact' | 'name' | undefined {
  if (!candidate.manifest.name) return undefined
  if (isExactMatch(candidate, query)) return 'exact'
  return isNameMatch(candidate, query) ? 'name' : undefined
}

function isExactMatch ({ candidateEditDir, stateValue, manifest }: EditDirCandidate, query: PatchDirQuery): boolean {
  const { userParam, parsedDep } = query
  if (parsedDep.bareSpecifier && stateValue.patchedPkg === userParam) return true
  const candidateNameVer = manifest.version ? `${manifest.name}@${manifest.version}` : manifest.name
  if (candidateNameVer === userParam) return true
  if (normalizePath(path.relative(query.patchesDir, candidateEditDir)) === query.normalizedUserParam) return true
  if (!parsedDep.alias || parsedDep.alias !== manifest.name || !parsedDep.bareSpecifier) return false
  return parsedDep.bareSpecifier === manifest.version ||
    Boolean(manifest.version && semver.satisfies(manifest.version, parsedDep.bareSpecifier))
}

function isNameMatch ({ stateValue, manifest }: EditDirCandidate, { userParam, parsedDep }: PatchDirQuery): boolean {
  return Boolean(parsedDep.alias && parsedDep.alias === manifest.name && !parsedDep.bareSpecifier) ||
    manifest.name === userParam ||
    stateValue.patchedPkg === userParam
}

function pickSingleMatch (matches: ResolvedPatchDir[], userParam: string): ResolvedPatchDir | undefined {
  if (matches.length === 1) {
    return matches[0]
  }
  if (matches.length > 1) {
    const list = matches.map(m => m.editDir).sort().map(editDir => `  ${editDir}`).join('\n')
    throw new PnpmError('AMBIGUOUS_PATCH_TARGET', `Found multiple patch directories for "${userParam}":\n${list}`, {
      hint: 'Specify the exact patch directory or version',
    })
  }
  return undefined
}

function invalidPatchDirError (userParam: string): PnpmError {
  return new PnpmError('INVALID_PATCH_DIR', `${userParam} is not a valid patch directory`, {
    hint: 'A valid patch directory should be created by `pnpm patch`',
  })
}
