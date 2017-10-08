import path = require('path')
import writeFileAtomicCB = require('write-file-atomic')
import thenify = require('thenify')
import rimraf = require('rimraf-then')
import yaml = require('js-yaml')
import {
  WANTED_SHRINKWRAP_FILENAME,
  CURRENT_SHRINKWRAP_FILENAME,
} from './constants'
import {Shrinkwrap} from './types'
import mkdirp = require('mkdirp-promise')
import logger from './logger'

const writeFileAtomic = thenify(writeFileAtomicCB)

const SHRINKWRAP_YAML_FORMAT = {
  sortKeys: true,
  lineWidth: 1000,
  noCompatMode: true,
}

export default function write (
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap
) {
  const wantedShrinkwrapPath = path.join(pkgPath, WANTED_SHRINKWRAP_FILENAME)
  const currentShrinkwrapPath = path.join(pkgPath, CURRENT_SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (Object.keys(wantedShrinkwrap.specifiers).length === 0) {
    return Promise.all([
      rimraf(wantedShrinkwrapPath),
      rimraf(currentShrinkwrapPath),
    ])
  }

  const yamlDoc = yaml.safeDump(wantedShrinkwrap, SHRINKWRAP_YAML_FORMAT)

  // in most cases the `shrinkwrap.yaml` and `node_modules/.shrinkwrap.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (wantedShrinkwrap === currentShrinkwrap) {
    return Promise.all([
      writeFileAtomic(wantedShrinkwrapPath, yamlDoc),
      mkdirp(path.dirname(currentShrinkwrapPath)).then(() => writeFileAtomic(currentShrinkwrapPath, yamlDoc)),
    ])
  }

  logger.warn('`shrinkwrap.yaml` differs from `node_modules/.shrinkwrap.yaml`. ' +
    'To fix this, run `pnpm install`. From pnpm version 2, named installations and uninstallations will fail ' +
    'when the content of `node_modules` won\'t match what the `shrinkwrap.yaml` expects.')

  const currentYamlDoc = yaml.safeDump(currentShrinkwrap, SHRINKWRAP_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(wantedShrinkwrapPath, yamlDoc),
    mkdirp(path.dirname(currentShrinkwrapPath)).then(() => writeFileAtomic(currentShrinkwrapPath, currentYamlDoc)),
  ])
}
