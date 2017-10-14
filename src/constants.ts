import path = require('path')

export const WANTED_SHRINKWRAP_FILENAME = 'shrinkwrap.yaml'
export const CURRENT_SHRINKWRAP_FILENAME = path.join('node_modules', '.shrinkwrap.yaml')

// Although .0 versions are supported, a bump to 3.1 would be a breaking change
// because comver version support was added after releasing version 3.
// From version 4.0 everything should be fine.
export const SHRINKWRAP_VERSION = 3

// The minor version as a perate field should be deprecated from v4
export const SHRINKWRAP_MINOR_VERSION = 1
