import getRegistryName = require('encode-registry') // TODO: remove this. BREAKING CHANGE
import fetch, {FetchedPackage, PackageContentInfo} from './fetch'
import pkgIdToFilename from './fs/pkgIdToFilename'
import {read, save, Store} from './fs/storeController'
import createGot, {Got} from './network/got'
import pkgIsUntouched from './pkgIsUntouched'
import resolve, {
  DirectoryResolution,
  PackageMeta,
  PackageSpec,
  Resolution,
} from './resolve'
import resolveStore from './resolveStore'

export {
  pkgIdToFilename,
  fetch,
  PackageContentInfo,
  FetchedPackage,
  resolve,
  PackageSpec,
  DirectoryResolution,
  Resolution,
  PackageMeta,
  Store,
  read,
  save,
  getRegistryName,
  createGot,
  Got,
  pkgIsUntouched,
  resolveStore,
}

export {
  ProgressLog,
  Log,
} from './loggers'
