import path = require('path')

export const WANTED_LOCKFILE = 'pnpm-lock.yaml'
export const CURRENT_LOCKFILE = path.join('node_modules', '.pnpm-lock.yaml')
export const LOCKFILE_VERSION = 5.1

export const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`
export const LAYOUT_VERSION = 3

export const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'
