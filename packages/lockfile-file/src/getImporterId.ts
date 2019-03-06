import normalize = require('normalize-path')
import path = require('path')

export default (lockfileDirectory: string, prefix: string) => normalize(path.relative(lockfileDirectory, prefix)) || '.'
