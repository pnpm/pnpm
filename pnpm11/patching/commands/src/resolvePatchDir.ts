import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import normalizePath from 'normalize-path'
import semver from 'semver'

import { type EditDirState, readEditDirState, readStateFile } from './stateFile.js'

export interface ResolvePatchDirOptions {
  dir: string
  lockfileDir: string
  modulesDir: string
}

export interface ResolvedPatchDir {
  editDir: string
  stateValue: EditDirState
}

export async function resolvePatchDir (
  userParam: string,
  opts: ResolvePatchDirOptions
): Promise<ResolvedPatchDir> {
  const directPath = path.resolve(opts.dir, userParam)
  if (fs.existsSync(directPath) && fs.statSync(directPath).isDirectory()) {
    const stateValue = readEditDirState({
      editDir: directPath,
      modulesDir: opts.modulesDir,
    })
    if (stateValue) {
      return { editDir: directPath, stateValue }
    }
  }

  const directPathFromLockfileDir = path.resolve(opts.lockfileDir, userParam)
  if (directPathFromLockfileDir !== directPath && fs.existsSync(directPathFromLockfileDir) && fs.statSync(directPathFromLockfileDir).isDirectory()) {
    const stateValue = readEditDirState({
      editDir: directPathFromLockfileDir,
      modulesDir: opts.modulesDir,
    })
    if (stateValue) {
      return { editDir: directPathFromLockfileDir, stateValue }
    }
  }

  const state = readStateFile(opts.modulesDir)
  if (!state || Object.keys(state).length === 0) {
    throw new PnpmError('INVALID_PATCH_DIR', `${userParam} is not a valid patch directory`, {
      hint: 'A valid patch directory should be created by `pnpm patch`',
    })
  }

  const parsedDep = parseWantedDependency(userParam)
  const exactMatches: ResolvedPatchDir[] = []
  const nameMatches: ResolvedPatchDir[] = []

  const patchesDir = path.join(opts.modulesDir, '.pnpm_patches')

  const candidates = await Promise.all(
    Object.entries(state).map(async ([candidateEditDir, stateValue]) => {
      if (!fs.existsSync(candidateEditDir) || !fs.statSync(candidateEditDir).isDirectory()) {
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

  for (const candidate of candidates) {
    if (!candidate) continue
    const { candidateEditDir, stateValue, manifest } = candidate

    const manifestName = manifest.name
    const manifestVersion = manifest.version
    if (!manifestName) continue

    const candidateNameVer = manifestVersion ? `${manifestName}@${manifestVersion}` : manifestName
    const relPatchesDir = normalizePath(path.relative(patchesDir, candidateEditDir))
    const normalizedUserParam = normalizePath(userParam)

    let isExact = false
    let isName = false

    if (parsedDep.bareSpecifier && stateValue.patchedPkg === userParam) {
      isExact = true
    } else if (candidateNameVer === userParam || relPatchesDir === normalizedUserParam) {
      isExact = true
    } else if (parsedDep.alias && parsedDep.alias === manifestName) {
      if (parsedDep.bareSpecifier) {
        if (parsedDep.bareSpecifier === manifestVersion) {
          isExact = true
        } else if (manifestVersion && semver.satisfies(manifestVersion, parsedDep.bareSpecifier)) {
          isExact = true
        }
      } else {
        isName = true
      }
    }

    if (!isExact && (manifestName === userParam || stateValue.patchedPkg === userParam)) {
      isName = true
    }

    if (isExact) {
      exactMatches.push({ editDir: candidateEditDir, stateValue })
    } else if (isName) {
      nameMatches.push({ editDir: candidateEditDir, stateValue })
    }
  }

  if (exactMatches.length === 1) {
    return exactMatches[0]
  }

  if (exactMatches.length > 1) {
    const list = exactMatches.map(m => m.editDir).sort().map(d => `  ${d}`).join('\n')
    throw new PnpmError('AMBIGUOUS_PATCH_TARGET', `Found multiple patch directories for "${userParam}":\n${list}`, {
      hint: 'Specify the exact patch directory or version',
    })
  }

  if (nameMatches.length === 1) {
    return nameMatches[0]
  }

  if (nameMatches.length > 1) {
    const list = nameMatches.map(m => m.editDir).sort().map(d => `  ${d}`).join('\n')
    throw new PnpmError('AMBIGUOUS_PATCH_TARGET', `Found multiple patch directories for "${userParam}":\n${list}`, {
      hint: 'Specify the exact patch directory or version',
    })
  }

  throw new PnpmError('INVALID_PATCH_DIR', `${userParam} is not a valid patch directory`, {
    hint: 'A valid patch directory should be created by `pnpm patch`',
  })
}
