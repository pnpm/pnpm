import { type PackageScripts } from '@pnpm/types'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'
import { readdirSync } from 'fs'
import path from 'path'

export function getNearest (name: string, list?: readonly string[]) {
  return list && didYouMean(name, list ?? [], {
    returnType: ReturnTypeEnums.FIRST_CLOSEST_MATCH,
  })
}

function readProgramsFromDir(binDir: string): string[] {
  const list = readdirSync(binDir)
  if (process.platform !== 'win32') return list
  const executableExtensions = ['.cmd', '.bat', '.ps1', '.exe', '.com']
  return list.map((fullName) => {
    const { name, ext } = path.parse(fullName)
    return executableExtensions.includes(ext) ? name : fullName
  })
}

export function getNearestProgram (opts: {
  programName: string,
  dir: string,
  workspaceDir: string | undefined,
}) {
  try {
    const { programName, dir, workspaceDir } = opts
    const binDir = path.join(dir, 'node_modules', '.bin')
    const programList = readProgramsFromDir(binDir)
    if (workspaceDir && workspaceDir !== dir) {
      const workspaceBinDir = path.join(workspaceDir, 'node_modules', '.bin')
      programList.push(...readProgramsFromDir(workspaceBinDir))
    }
    return getNearest(programName, programList)
  } catch (_err) {
    return null
  }
}

export function getNearestScript (scriptName: string, scripts?: PackageScripts | undefined) {
  return getNearest(scriptName, scripts && Object.keys(scripts))
}

export function buildCommandNotFoundHint (scriptName: string, scripts?: PackageScripts | undefined) {
  let hint = `Command "${scriptName}" not found.`

  const nearestCommand = getNearestScript(scriptName, scripts)

  if (nearestCommand) {
    hint += ` Did you mean "pnpm run ${nearestCommand}"?`
  }

  return hint
}
