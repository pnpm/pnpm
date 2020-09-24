import path = require('path')
import normalize = require('normalize-path')

export default (lockfileDir: string, prefix: string): string => normalize(path.relative(lockfileDir, prefix)) || '.'
