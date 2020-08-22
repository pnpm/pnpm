import { PREFIX_MAX_LENGTH } from '../outputConstants'
import path = require('path')
import normalize = require('normalize-path')

export default function formatPrefix (cwd: string, prefix: string) {
  prefix = formatPrefixNoTrim(cwd, prefix)

  if (prefix.length <= PREFIX_MAX_LENGTH) {
    return prefix
  }

  const shortPrefix = prefix.substr(-PREFIX_MAX_LENGTH + 3)

  const separatorLocation = shortPrefix.indexOf('/')

  if (separatorLocation <= 0) {
    return `...${shortPrefix}`
  }

  return `...${shortPrefix.substr(separatorLocation)}`
}

export function formatPrefixNoTrim (cwd: string, prefix: string) {
  return normalize(path.relative(cwd, prefix) || '.')
}
