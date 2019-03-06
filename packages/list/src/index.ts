import { Registries } from '@pnpm/types'
import npa = require('@zkochan/npm-package-arg')
import dh, {
  forPackages as dhForPackages,
  PackageSelector,
} from 'dependencies-hierarchy'
import path = require('path')
import readPkg from './readPkg'
import renderParseable from './renderParseable'
import renderTree from './renderTree'

const DEFAULTS = {
  alwaysPrintRootPackage: true,
  depth: 0,
  long: false,
  only: undefined,
  parseable: false,
  registries: undefined,
}

export async function forPackages (
  packages: string[],
  projectPath: string,
  maybeOpts?: {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory?: string,
    long?: boolean,
    only?: 'dev' | 'prod',
    parseable?: boolean,
    registries?: Registries,
  },
) {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const searched: PackageSelector[] = packages.map((arg) => {
    const parsed = npa(arg)
    if (parsed.raw === parsed.name) {
      return parsed.name
    }
    if (parsed.type !== 'version' && parsed.type !== 'range') {
      throw new Error(`Invalid argument - ${arg}. List can search only by version or range`)
    }
    return {
      name: parsed.name,
      range: parsed.fetchSpec,
    }
  })

  const tree = await dhForPackages(searched, projectPath, {
    depth: opts.depth,
    lockfileDirectory: maybeOpts && maybeOpts.lockfileDirectory,
    only: opts.only,
    registries: opts.registries,
  })

  const print = getPrinter(opts.parseable)
  const entryPkg = await readPkg(path.resolve(projectPath, 'package.json'))
  return print({
    name: entryPkg.name,
    path: projectPath,
    version: entryPkg.version,
  }, tree, {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    long: opts.long,
  })
}

export default async function (
  projectPath: string,
  maybeOpts?: {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory?: string,
    long?: boolean,
    only?: 'dev' | 'prod',
    parseable?: boolean,
    registries?: Registries,
  },
) {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const tree = opts.depth === -1
    ? []
    : await dh(projectPath, {
      depth: opts.depth,
      lockfileDirectory: maybeOpts && maybeOpts.lockfileDirectory,
      only: opts.only,
      registries: opts.registries,
    })

  const print = getPrinter(opts.parseable)
  const entryPkg = await readPkg(path.resolve(projectPath, 'package.json'))
  return print({
    name: entryPkg.name,
    path: projectPath,
    version: entryPkg.version,
  }, tree, {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    long: opts.long,
  })
}

function getPrinter (parseable: boolean) {
  if (parseable) return renderParseable
  return renderTree
}
