import dh, {
  forPackages as dhForPackages,
  PackageNode,
  PackageSelector,
} from 'dependencies-hierarchy'
import archy = require('archy')
import readPkgCB = require('read-package-json')
import thenify = require('thenify')
import npa = require('npm-package-arg')
import pLimit = require('p-limit')
import path = require('path')
import chalk = require('chalk')

const limitPkgReads = pLimit(4)
const _readPkg = thenify(readPkgCB)
const readPkg = (pkgPath: string) => limitPkgReads(() => _readPkg(pkgPath))

export async function forPackages (
  packages: string[],
  projectPath: string,
  opts?: {
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
  }
) {
  const _opts = Object.assign({}, {
    depth: 0,
    long: false,
    only: undefined,
  }, opts)

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

  return _list(projectPath, tree, {
    long: _opts.long,
  })
}

export default async function (
  projectPath: string,
  opts?: {
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
  }
) {
  const _opts = Object.assign({}, {
    depth: 0,
    long: false,
    only: undefined,
  }, opts)

  const tree = await dh(projectPath, {
    depth: _opts.depth,
    only: _opts.only,
  })

  return _list(projectPath, tree, {
    long: _opts.long,
  })
}

async function _list (
  projectPath: string,
  tree: PackageNode[],
  opts: {
    long: boolean,
  }
) {
  const pkg = await readPkg('package.json')

  const s = archy({
    label: `${pkg.name}@${pkg.version} ${projectPath}`,
    nodes: await toArchyTree(tree, {
      long: opts.long,
      modules: path.join(projectPath, 'node_modules')
    }),
  })

  return s
}

async function toArchyTree (
  nodes: PackageNode[],
  opts: {
    long: boolean,
    modules: string,
  }
): Promise<archy.Data[]> {
  return Promise.all(
    nodes.map(async node => {
      const nodes = await toArchyTree(node.dependencies || [], opts)
      if (opts.long) {
        const pkg = await readPkg(path.join(opts.modules, `.${node.pkg.path}`, 'node_modules', node.pkg.name, 'package.json'))
        const labelLines = [
          printLabel(node),
          pkg.description
        ]
        if (pkg.repository) {
          labelLines.push(pkg.repository.url)
        }
        if (pkg.homepage) {
          labelLines.push(pkg.homepage)
        }
        return {
          label: labelLines.join('\n'),
          nodes,
        }
      }
      return {
        label: printLabel(node),
        nodes,
      }
    })
  )
}

function printLabel (node: PackageNode) {
  const txt = `${node.pkg.name}@${node.pkg.version}`
  if (node.searched) {
    return chalk.yellow.bgBlack(txt)
  }
  return txt
}
