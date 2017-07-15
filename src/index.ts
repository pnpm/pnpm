import dh, {
  forPackages as dhForPackages,
  PackageSelector,
} from 'dependencies-hierarchy'
import npa = require('npm-package-arg')
import printTree from './printTree'
import printParseable from './printParseable'

const DEFAULTS = {
  depth: 0,
  long: false,
  parseable: false,
  only: undefined,
}

export async function forPackages (
  packages: string[],
  projectPath: string,
  opts?: {
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
    parseable?: boolean,
  }
) {
  const _opts = Object.assign({}, DEFAULTS, opts)

  const searched: PackageSelector[] = packages.map(arg => {
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
    depth: _opts.depth,
    only: _opts.only,
  })

  const print = getPrinter(_opts.parseable)
  return print(projectPath, tree, {
    long: _opts.long,
  })
}

export default async function (
  projectPath: string,
  opts?: {
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
    parseable?: boolean,
  }
) {
  const _opts = Object.assign({}, DEFAULTS, opts)

  const tree = await dh(projectPath, {
    depth: _opts.depth,
    only: _opts.only,
  })

  const print = getPrinter(_opts.parseable)
  return print(projectPath, tree, {
    long: _opts.long,
  })
}

function getPrinter (parseable: boolean) {
  if (parseable) return printParseable
  return printTree
}
