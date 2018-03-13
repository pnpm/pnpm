import yaml = require('js-yaml')
import mkdirp = require('mkdirp-promise')
import path = require('path')
import R = require('ramda')
import rimraf = require('rimraf-then')
import promisify = require('util.promisify')
import writeFileAtomicCB = require('write-file-atomic')
import {
  CURRENT_SHRINKWRAP_FILENAME,
  WANTED_SHRINKWRAP_FILENAME,
} from './constants'
import logger from './logger'
import {Shrinkwrap} from './types'

const writeFileAtomic = promisify(writeFileAtomicCB)

const SHRINKWRAP_YAML_FORMAT = {
  lineWidth: 1000,
  noCompatMode: true,
  noRefs: true,
  sortKeys: true,
}

export function writeWantedOnly (
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
) {
  return writeShrinkwrap(WANTED_SHRINKWRAP_FILENAME, pkgPath, wantedShrinkwrap)
}

export function writeCurrentOnly (
  pkgPath: string,
  currentShrinkwrap: Shrinkwrap,
) {
  return writeShrinkwrap(CURRENT_SHRINKWRAP_FILENAME, pkgPath, currentShrinkwrap)
}

function writeShrinkwrap (
  shrinkwrapFilename: string,
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
) {
  const shrinkwrapPath = path.join(pkgPath, shrinkwrapFilename)

  // empty shrinkwrap is not saved
  if (R.isEmpty(wantedShrinkwrap.specifiers || {}) && R.isEmpty(wantedShrinkwrap.dependencies || {})) {
    return rimraf(shrinkwrapPath)
  }

  const yamlDoc = yaml.safeDump(wantedShrinkwrap, SHRINKWRAP_YAML_FORMAT)

  return writeFileAtomic(shrinkwrapPath, yamlDoc)
}

export default function write (
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap,
) {
  const wantedShrinkwrapPath = path.join(pkgPath, WANTED_SHRINKWRAP_FILENAME)
  const currentShrinkwrapPath = path.join(pkgPath, CURRENT_SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (R.isEmpty(wantedShrinkwrap.specifiers || {}) && R.isEmpty(wantedShrinkwrap.dependencies || {})) {
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
