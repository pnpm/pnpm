import path = require('path')
import writeFileAtomicCB = require('write-file-atomic')
import thenify = require('thenify')
import rimraf = require('rimraf-then')
import yaml = require('js-yaml')
import {SHRINKWRAP_FILENAME, PRIVATE_SHRINKWRAP_FILENAME} from './constants'
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
  shrinkwrap: Shrinkwrap,
  privateShrinkwrap: Shrinkwrap
) {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  const privateShrinkwrapPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (Object.keys(shrinkwrap.specifiers).length === 0) {
    return Promise.all([
      rimraf(shrinkwrapPath),
      rimraf(privateShrinkwrapPath),
    ])
  }

  const yamlDoc = yaml.safeDump(shrinkwrap, SHRINKWRAP_YAML_FORMAT)

  // in most cases the `shrinkwrap.yaml` and `node_modules/.shrinkwrap.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (shrinkwrap === privateShrinkwrap) {
    return Promise.all([
      writeFileAtomic(shrinkwrapPath, yamlDoc),
      mkdirp(path.dirname(privateShrinkwrapPath)).then(() => writeFileAtomic(privateShrinkwrapPath, yamlDoc)),
    ])
  }

  logger.warn('`shrinkwrap.yaml` differs from `node_modules/.shrinkwrap.yaml`. ' +
    'To fix this, run `pnpm install`. From pnpm version 2, named installations and uninstallations will fail ' +
    'when the content of `node_modules` won\'t match what the `shrinkwrap.yaml` expects.')

  const privateYamlDoc = yaml.safeDump(privateShrinkwrap, SHRINKWRAP_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(shrinkwrapPath, yamlDoc),
    mkdirp(path.dirname(privateShrinkwrapPath)).then(() => writeFileAtomic(privateShrinkwrapPath, privateYamlDoc)),
  ])
}
