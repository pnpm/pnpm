import path = require('path')

export const WANTED_SHRINKWRAP_FILENAME = 'pnpm-lock.yaml'
export const CURRENT_SHRINKWRAP_FILENAME = path.join('node_modules', '.pnpm-lock.yaml')
export const SHRINKWRAP_VERSION = 5

export const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`
export const LAYOUT_VERSION = 1

export const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'
