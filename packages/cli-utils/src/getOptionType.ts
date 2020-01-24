import { CompletionCtx } from '@pnpm/command'
import nopt = require('nopt')
import R = require('ramda')

export function getOptionCompletions (
  optionTypes: Record<string, Object>,
  shortHands: Record<string, string | string[]>,
  option: string,
) {
  const optionType = getOptionType(optionTypes, shortHands, option)
  return optionTypeToCompletion(optionType)
}

function optionTypeToCompletion (optionType: Object): undefined | string[] {
  switch (optionType) {
    // In this case the option is complete
    case undefined:
    case Boolean: return undefined
    // In this case, anything may be the option value
    case String:
    case Number: return []
  }
  if (!Array.isArray(optionType)) return []
  if (optionType.length === 1) {
    return optionTypeToCompletion(optionType)
  }
  return optionType.filter((ot) => typeof ot === 'string')
}

function getOptionType (
  optionTypes: Record<string, Object>,
  shortHands: Record<string, string | string[]>,
  option: string,
) {
  const allBools = R.fromPairs(Object.entries(optionTypes).map(([optionName]) => [optionName, Boolean]))
  const result = nopt(allBools, shortHands, [option], 0)
  delete result.argv
  return optionTypes[Object.entries(result)[0]?.[0]]
}

export function getLastOption (completionCtx: CompletionCtx) {
  if (isOption(completionCtx.prev)) return completionCtx.prev
  if (completionCtx.lastPartial === '' || completionCtx.words <= 1) return null
  const words = completionCtx.line.slice(0, completionCtx.point).trim().split(/\s+/)
  const lastWord = words[words.length - 2]
  return isOption(lastWord) ? lastWord : null
}

function isOption (word: string) {
  return word.startsWith('--') && word.length >= 3 ||
    word.startsWith('-') && word.length >= 2
}
