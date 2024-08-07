import path from 'path'
import { type ParseWantedDependencyResult } from '@pnpm/parse-wanted-dependency'

export interface GetEditDirOptions {
  modulesDir: string
}

export function getEditDirPath (param: string, patchedDep: ParseWantedDependencyResult, opts: GetEditDirOptions): string {
  const editDirName = getEditDirNameFromParsedDep(patchedDep) ?? param
  return path.join(opts.modulesDir, '.pnpm_patches', editDirName)
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
