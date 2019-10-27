import normalize = require('normalize-path')
import path = require('path')

export default (lockfileDir: string, prefix: string): string => normalize(path.relative(lockfileDir, prefix)) || '.'
