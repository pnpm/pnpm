import { Registries } from '@pnpm/types'
import dh, {
  forPackages as dhForPackages,
  PackageSelector,
} from 'dependencies-hierarchy'
import npa = require('npm-package-arg')
import printParseable from './printParseable'
import printTree from './printTree'

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
    long?: boolean,
    only?: 'dev' | 'prod',
    parseable?: boolean,
    registries?: Registries,
    shrinkwrapDirectory?: string,
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
    only: opts.only,
    registries: opts.registries,
    shrinkwrapDirectory: maybeOpts && maybeOpts.shrinkwrapDirectory,
  })

  const print = getPrinter(opts.parseable)
  return print(projectPath, tree, {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    long: opts.long,
  })
}

export default async function (
  projectPath: string,
  maybeOpts?: {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    long?: boolean,
    only?: 'dev' | 'prod',
    parseable?: boolean,
    registries?: Registries,
    shrinkwrapDirectory?: string,
  },
) {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const tree = await dh(projectPath, {
    depth: opts.depth,
    only: opts.only,
    registries: opts.registries,
    shrinkwrapDirectory: maybeOpts && maybeOpts.shrinkwrapDirectory,
  })

  const print = getPrinter(opts.parseable)
  return print(projectPath, tree, {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    long: opts.long,
  })
}

function getPrinter (parseable: boolean) {
  if (parseable) return printParseable
  return printTree
}
