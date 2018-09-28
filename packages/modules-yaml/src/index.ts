import { DependenciesField } from '@pnpm/types'
import loadYamlFile = require('load-yaml-file')
import path = require('path')
import writeYamlFile = require('write-yaml-file')

// The dot prefix is needed because otherwise `npm shrinkwrap`
// thinks that it is an extraneous package.
const MODULES_FILENAME = '.modules.yaml'

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export interface Modules {
  importers: {
    [importerPath: string]: {
      hoistedAliases: {[depPath: string]: string[]}
      shamefullyFlatten: boolean,
    },
  },
  included: IncludedDependencies,
  independentLeaves: boolean,
  layoutVersion: number,
  packageManager: string,
  pendingBuilds: string[],
  skipped: string[],
  store: string,
}

export async function read (virtualStoreDir: string): Promise<Modules | null> {
  const modulesYamlPath = path.join(virtualStoreDir, MODULES_FILENAME)
  try {
    const m = await loadYamlFile<Modules>(modulesYamlPath)
    // for backward compatibility
    // tslint:disable:no-string-literal
    if (m['storePath']) {
      m.store = m['storePath']
      delete m['storePath']
    }
    if (!m.importers) {
      m.importers = {
        '.': {
          hoistedAliases: m['hoistedAliases'],
          shamefullyFlatten: m['shamefullyFlatten'],
        },
      }
      delete m['hoistedAliases']
      delete m['shamefullyFlatten']
    }
    // tslint:enable:no-string-literal
    return m
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return null
  }
}

const YAML_OPTS = {sortKeys: true}

export function write (
  virtualStoreDir: string,
  modules: Modules,
) {
  const modulesYamlPath = path.join(virtualStoreDir, MODULES_FILENAME)
  if (modules['skipped']) modules['skipped'].sort() // tslint:disable-line:no-string-literal

  return writeYamlFile(modulesYamlPath, normalizeModules(modules), YAML_OPTS)
}

function normalizeModules (m: Modules) {
  const normalized = {...m}
  if (Object.keys(m.importers).length === 1 && m.importers['.']) {
    Object.assign(normalized, m.importers['.'])
    delete normalized.importers
  }
  return normalized
}
