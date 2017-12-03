import getRegistryName = require('encode-registry') // TODO: remove this. BREAKING CHANGE
import {
  FetchFunction,
  FetchOptions,
} from './combineFetchers'
import createFetcher, {FetchedPackage, PackageContentInfo} from './fetch'
import pkgIdToFilename from './fs/pkgIdToFilename'
import {read, save, Store} from './fs/storeController'
import pkgIsUntouched from './pkgIsUntouched'
import {
  DirectoryResolution,
  Resolution,
  ResolveFunction,
  ResolveOptions,
} from './resolvers'
import resolveStore from './resolveStore'

export {
  pkgIdToFilename,
  createFetcher,
  PackageContentInfo,
  FetchedPackage,
  DirectoryResolution,
  Resolution,
  Store,
  read,
  save,
  getRegistryName,
  pkgIsUntouched,
  resolveStore,
  ResolveFunction,
  ResolveOptions,
  FetchFunction,
  FetchOptions,
}

export {
  ProgressLog,
  Log,
} from './loggers'
