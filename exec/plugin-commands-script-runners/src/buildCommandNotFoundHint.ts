import { type PackageScripts } from '@pnpm/types'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'

export function getNearestCommand (scriptName: string, scripts?: PackageScripts | undefined) {
  return scripts && didYouMean(scriptName, Object.keys(scripts), {
    returnType: ReturnTypeEnums.FIRST_CLOSEST_MATCH,
  })
}

export function buildCommandNotFoundHint (scriptName: string, scripts?: PackageScripts | undefined) {
  let hint = `Command "${scriptName}" not found.`

  const nearestCommand = getNearestCommand(scriptName, scripts)

  if (nearestCommand) {
    hint += ` Did you mean "pnpm run ${nearestCommand}"?`
  }

  return hint
}
