import { type PackageScripts } from '@pnpm/types'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'
import { readdirSync } from 'fs'
import path from 'path'

export function getNearestProgram ({
  dir,
  modulesDir,
  programName,
  workspaceDir,
}: {
  dir: string
  modulesDir: string
  programName: string
  workspaceDir: string | undefined
}): string | null {
  try {
    const binDir = path.join(dir, modulesDir, '.bin')
    const programList = readProgramsFromDir(binDir)
    if (workspaceDir && workspaceDir !== dir) {
      const workspaceBinDir = path.join(workspaceDir, modulesDir, '.bin')
      programList.push(...readProgramsFromDir(workspaceBinDir))
    }
    return getNearest(programName, programList)
  } catch {
    return null
  }
}

function readProgramsFromDir (binDir: string): string[] {
  const files = readdirSync(binDir)
  if (process.platform !== 'win32') return files
  const executableExtensions = ['.cmd', '.bat', '.ps1', '.exe', '.com']
  return files.map((fullName) => {
    const { name, ext } = path.parse(fullName)
    return executableExtensions.includes(ext.toLowerCase()) ? name : fullName
  })
}

export function buildCommandNotFoundHint (scriptName: string, scripts?: PackageScripts | undefined): string {
  let hint = `Command "${scriptName}" not found.`

  const nearestCommand = getNearestScript(scriptName, scripts)

  if (nearestCommand) {
    hint += ` Did you mean "pnpm run ${nearestCommand}"?`
  }

  return hint
}

export function getNearestScript (scriptName: string, scripts?: PackageScripts | undefined): string | null {
  return getNearest(scriptName, Object.keys(scripts ?? []))
}

export function getNearest (name: string, list: readonly string[]): string | null {
  if (list == null || list.length === 0) return null
  return didYouMean(name, list, {
    returnType: ReturnTypeEnums.FIRST_CLOSEST_MATCH,
  })
}
