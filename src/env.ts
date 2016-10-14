import semver = require('semver')

export const preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')
export const isWindows = process.platform === 'win32'
