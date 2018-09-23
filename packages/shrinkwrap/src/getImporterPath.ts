import normalize = require('normalize-path')
import path = require('path')

export default (shrinkwrapDirectory: string, prefix: string) => normalize(path.relative(shrinkwrapDirectory, prefix)) || '.'
