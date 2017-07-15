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

export default async function (
  projectPath: string,
  args: string[],
  opts: {
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
  }
) {
  const searched: PackageSelector[] = args.map(arg => {
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

  const hopts = {
    depth: opts.depth || 0,
    only: opts.only,
  }

  const tree = searched.length
    ? await dhForPackages(searched, projectPath, hopts)
    : await dh(projectPath, hopts)

  const pkg = await readPkg('package.json')

  const s = archy({
    label: `${pkg.name}@${pkg.version} ${projectPath}`,
    nodes: await toArchyTree(tree, {
      long: Boolean(opts.long),
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
