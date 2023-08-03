import { type PackageScripts } from '@pnpm/types'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'
import { readdir } from 'fs/promises'
import path from 'path'

export function getNearest (name: string, list?: readonly string[]) {
  return list && didYouMean(name, list ?? [], {
    returnType: ReturnTypeEnums.FIRST_CLOSEST_MATCH,
  })
}

export async function getNearestProgram (programName: string) {
  try {
    const binDir = path.join(process.cwd(), 'node_modules', '.bin')
    const programList = await readdir(binDir)
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
