import normalize = require('normalize-path')
import path = require('path')
import { PREFIX_MAX_LENGTH } from '../outputConstants'

export default function formatPrefix (cwd: string, prefix: string) {
  prefix = normalize(path.relative(cwd, prefix) || '.')

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
