import { type PackageScripts } from '@pnpm/types'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'
import { readdir } from 'fs/promises'
import path from 'path'

export function getNearestCommand (name: string, list?: readonly string[]) {
  return list && didYouMean(name, list || [], {
    returnType: ReturnTypeEnums.FIRST_CLOSEST_MATCH,
  })
}

export async function buildProgramNotFoundHint (programName: string) {
  const binDir = path.join(process.cwd(), 'node_modules', '.bin')
  const programList = await readdir(binDir)
  return getNearestCommand(programName, programList)
}

export function buildCommandNotFoundHint (scriptName: string, scripts?: PackageScripts | undefined) {
  let hint = `Command "${scriptName}" not found.`

  const nearestCommand = getNearestCommand(scriptName, scripts && Object.keys(scripts))

  if (nearestCommand) {
    hint += ` Did you mean "pnpm run ${nearestCommand}"?`
  }

  return hint
}
