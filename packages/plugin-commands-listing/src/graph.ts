import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config } from '@pnpm/config'
import { ProjectsGraph } from '@pnpm/types'
import sortPackages from '@pnpm/sort-packages'
import graphviz from 'graphviz'
import PnpmError from '@pnpm/error'
import renderHelp from 'render-help'

export function rcOptionsTypes () {
  return {}
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
  chunks: Boolean,
})

export const commandNames = ['graph']

export function help () {
  return renderHelp({
    aliases: ['graphs'],
    description: 'Output a graph of packages and their dependencies',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Perform command on every package in subdirectories \
or on every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Include chunks in the graph',
            name: '--chunks',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('graph'),
    usages: [
      'pnpm graph',
    ],
  })
}

export type GraphCommandOptions = Pick<Config,
| 'color'
| 'recursive'
| 'selectedProjectsGraph'
| 'workspaceDir'
> & Required<Pick<Config, 'selectedProjectsGraph'>>
& Partial<Pick<Config, 'cliOptions'>> & {
  chunks?: boolean
}

export async function handler (opts: GraphCommandOptions): Promise<string> {
  if (!opts.recursive) {
    throw new PnpmError('GRAPH_NOT_RECURSIVE', 'The "pnpm graph" command currently only works with the "-r" option')
  }

  if (!opts.workspaceDir) {
    throw new PnpmError('WORKSPACE_OPTION_OUTSIDE_WORKSPACE', 'The "pnpm graph" command can only be used inside a workspace')
  }

  const graph = relativizeGraph(opts.workspaceDir, opts.selectedProjectsGraph)

  const g = graphviz.digraph('G')

  if (opts.chunks) {
    const chunks = sortPackages(graph)

    for (const [i, chunk] of chunks.entries()) {
      // Graphviz will only render clusters if they start with `cluster`
      const cluster = g.addCluster(`cluster_${i}`)

      cluster.set('label', `Chunk #${i}`)
      cluster.set('labeljust', 'l')
      if (opts.color !== 'never') {
        cluster.set('color', 'blue')
      }

      for (const pkgPath of chunk) {
        cluster.addNode(pkgPath)

        for (const dependency of graph[pkgPath].dependencies) {
          g.addEdge(pkgPath, dependency)
        }
      }
    }
  } else {
    for (const [pkgPath, pkg] of Object.entries(graph)) {
      g.addNode(pkgPath)
      for (const dependency of pkg.dependencies) {
        g.addEdge(pkgPath, dependency)
      }
    }
  }

  return g.to_dot()
}

function relativizeGraph (workspaceDir: string, graph: ProjectsGraph) {
  return Object.fromEntries(Object.entries(graph).map(([k, v]) => [
    relativizePath(workspaceDir, k),
    { ...v, dependencies: v.dependencies.map(d => relativizePath(workspaceDir, d)) },
  ]))
}

function relativizePath (workspaceDir: string, absolutePath: string) {
  const relative = path.relative(workspaceDir, absolutePath)
  return relative === '' ? '.' : relative
}
