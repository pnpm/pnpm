import path = require('path')

export const SHRINKWRAP_FILENAME = 'shrinkwrap.yaml'
export const PRIVATE_SHRINKWRAP_FILENAME = path.join('node_modules', '.shrinkwrap.yaml')

// Although .0 versions are supported, a bump to 3.1 would be a breaking change
// because comver version support was added after releasing version 3.
// From version 4.0 everything should be fine.
export const SHRINKWRAP_VERSION = 3
