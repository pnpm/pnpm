import { Shrinkwrap } from '@pnpm/shrinkwrap-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
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
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  return writeShrinkwrap(WANTED_SHRINKWRAP_FILENAME, pkgPath, wantedShrinkwrap, opts)
}

export function writeCurrentOnly (
  pkgPath: string,
  currentShrinkwrap: Shrinkwrap,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  return writeShrinkwrap(CURRENT_SHRINKWRAP_FILENAME, pkgPath, currentShrinkwrap, opts)
}

function writeShrinkwrap (
  shrinkwrapFilename: string,
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  const shrinkwrapPath = path.join(pkgPath, shrinkwrapFilename)

  // empty shrinkwrap is not saved
  if (isEmptyShrinkwrap(wantedShrinkwrap)) {
    return rimraf(shrinkwrapPath)
  }

  const yamlDoc = yaml.safeDump(normalizeShrinkwrap(wantedShrinkwrap, opts && opts.forceSharedFormat === true || false), SHRINKWRAP_YAML_FORMAT)

  return writeFileAtomic(shrinkwrapPath, yamlDoc)
}

function isEmptyShrinkwrap (shr: Shrinkwrap) {
  return R.values(shr.importers).every((importer) => R.isEmpty(importer.specifiers || {}) && R.isEmpty(importer.dependencies || {}))
}

function normalizeShrinkwrap (shr: Shrinkwrap, forceSharedFormat: boolean) {
  if (forceSharedFormat === false && R.equals(R.keys(shr.importers), ['.'])) {
    const shrToSave = {
      ...shr,
      ...shr.importers['.'],
    }
    delete shrToSave.importers
    for (const depType of DEPENDENCIES_FIELDS) {
      if (R.isEmpty(shrToSave[depType])) {
        delete shrToSave[depType]
      }
    }
    if (R.isEmpty(shrToSave.packages)) {
      delete shrToSave.packages
    }
    return normalizeShrinkwrapVersion(shrToSave)
  } else {
    const shrToSave = {
      ...shr,
      importers: R.keys(shr.importers).reduce((acc, alias) => {
        const importer = shr.importers[alias]
        const normalizedImporter = {
          specifiers: importer.specifiers,
        }
        for (const depType of DEPENDENCIES_FIELDS) {
          if (!R.isEmpty(importer[depType] || {})) {
            normalizedImporter[depType] = importer[depType]
          }
        }
        acc[alias] = normalizedImporter
        return acc
      }, {}),
    }
    if (R.isEmpty(shrToSave.packages)) {
      delete shrToSave.packages
    }
    delete shrToSave.registry
    return normalizeShrinkwrapVersion(shrToSave)
  }
}

function normalizeShrinkwrapVersion (shr: Shrinkwrap) {
  if (shr.shrinkwrapVersion < 4 && shr.shrinkwrapVersion.toString().indexOf('.') !== -1) {
    const [majorVersion, minorVersion] = shr.shrinkwrapVersion.toString().split('.').map((v) => parseInt(v, 10))
    if (minorVersion > 0) {
      shr['shrinkwrapMinorVersion'] = minorVersion
    }
    shr.shrinkwrapVersion = majorVersion
  }
  return shr
}

export default function write (
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  const wantedShrinkwrapPath = path.join(pkgPath, WANTED_SHRINKWRAP_FILENAME)
  const currentShrinkwrapPath = path.join(pkgPath, CURRENT_SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (isEmptyShrinkwrap(wantedShrinkwrap)) {
    return Promise.all([
      rimraf(wantedShrinkwrapPath),
      rimraf(currentShrinkwrapPath),
    ])
  }

  const forceSharedFormat = opts && opts.forceSharedFormat === true || false
  const yamlDoc = yaml.safeDump(normalizeShrinkwrap(wantedShrinkwrap, forceSharedFormat), SHRINKWRAP_YAML_FORMAT)

  // in most cases the `shrinkwrap.yaml` and `node_modules/.shrinkwrap.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (wantedShrinkwrap === currentShrinkwrap) {
    return Promise.all([
      writeFileAtomic(wantedShrinkwrapPath, yamlDoc),
      mkdirp(path.dirname(currentShrinkwrapPath)).then(() => writeFileAtomic(currentShrinkwrapPath, yamlDoc)),
    ])
  }

  logger.debug({
    message: '`shrinkwrap.yaml` differs from `node_modules/.shrinkwrap.yaml`',
    prefix: pkgPath,
  })

  const currentYamlDoc = yaml.safeDump(normalizeShrinkwrap(currentShrinkwrap, forceSharedFormat), SHRINKWRAP_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(wantedShrinkwrapPath, yamlDoc),
    mkdirp(path.dirname(currentShrinkwrapPath)).then(() => writeFileAtomic(currentShrinkwrapPath, currentYamlDoc)),
  ])
}
