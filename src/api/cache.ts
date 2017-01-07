import path = require('path')
import rimraf = require('rimraf-then')
import {DEFAULT_GLOBAL_PATH} from './constantDefaults'
import expandTilde from '../fs/expandTilde'

export function cleanCache (globalPath?: string) {
  globalPath = globalPath || DEFAULT_GLOBAL_PATH
  const cachePath = getCachePath(globalPath)
  return rimraf(cachePath)
}

export function getCachePath (globalPath: string) {
  return path.join(expandTilde(globalPath), 'cache')
}
