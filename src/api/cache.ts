import path = require('path')
import rimraf = require('rimraf-then')
import {GlobalPath as DefaultGlobalPath} from './constantDefaults'
import expandTilde from '../fs/expandTilde'

export function cleanCache (globalPath?: string) {
  globalPath = globalPath || DefaultGlobalPath
  const cachePath = getCachePath(globalPath)
  return rimraf(cachePath)
}

export function getCachePath (globalPath: string) {
  return path.join(expandTilde(globalPath), 'cache')
}
