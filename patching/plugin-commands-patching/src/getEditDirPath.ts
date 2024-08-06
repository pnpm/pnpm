import path from 'path'
import { type ParseWantedDependencyResult } from '@pnpm/parse-wanted-dependency'

export interface GetEditDirOptions {
  editDir?: string
  modulesDir?: string
}

export function getEditDirPath (param: string, patchedDep: ParseWantedDependencyResult, opts?: GetEditDirOptions): string {
  if (opts?.editDir) return opts.editDir
  const modulesDir = opts?.modulesDir ?? 'node_modules'
  const editDirName = getEditDirNameFromParsedDep(patchedDep) ?? param
  return path.join(modulesDir, '.pnpm_patches', editDirName)
}

function getEditDirNameFromParsedDep (patchedDep: ParseWantedDependencyResult): string | undefined {
  if (patchedDep.alias && patchedDep.pref) {
    const pref = patchedDep.pref.replace(/[\\/:*?"<>|]+/g, '+')
    return `${patchedDep.alias}@${pref}`
  }

  if (patchedDep.alias) {
    return patchedDep.alias
  }

  return undefined
}
