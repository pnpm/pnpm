import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import prepare, { preparePackages } from '@pnpm/prepare'
import stripAnsi from 'strip-ansi'
import { graph } from '../src'

test('`pnpm graph` should fail if not recursive', async () => {
  prepare()

  let err!: PnpmError
  try {
    await graph.handler({
      color: 'auto',
      recursive: false,
      workspaceDir: process.cwd(),
      selectedProjectsGraph: {},
    })
  } catch (_err) {
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_GRAPH_NOT_RECURSIVE')
  expect(err.message).toMatch(/The "pnpm graph" command currently only works with the "-r" option/)
})

test('`pnpm graph` should fail if not in a workspace', async () => {
  prepare()

  let err!: PnpmError
  try {
    await graph.handler({
      color: 'auto',
      recursive: true,
      selectedProjectsGraph: {},
    })
  } catch (_err) {
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_WORKSPACE_OPTION_OUTSIDE_WORKSPACE')
  expect(err.message).toMatch(/The "pnpm graph" command can only be used inside a workspace/)
})

describe('common dependencies', () => {
  beforeAll(() => {
    preparePackages([
      {
        name: 'project-dev',
        version: '1.0.0',

        devDependencies: {
          'project-common': 'workspace:1.0.0',
        },
      },
      {
        name: 'project-prod',
        version: '1.0.0',

        dependencies: {
          'project-common': 'workspace:1.0.0',
        },
      },
      {
        name: 'project-optional',
        version: '1.0.0',

        optionalDependencies: {
          'project-common': 'workspace:1.0.0',
        },
      },
      {
        name: 'project-peer',
        version: '1.0.0',

        peerDependencies: {
          'project-common': 'workspace:1.0.0',
        },
      },
      {
        name: 'project-common',
        version: '1.0.0',
      },
    ])
  })

  test('"graph" should find all dependencies', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'auto',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
  "project-common";
  "project-dev";
  "project-optional";
  "project-peer";
  "project-prod";
  "project-dev" -> "project-common";
  "project-optional" -> "project-common";
  "project-prod" -> "project-common";
}
`)

    expect(stripAnsi(output)).not.toContain(`digraph G {
    "project-peer" -> "project-common";
  }
  `)
  })

  test('"chunks" should give clusters', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'auto',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
      chunks: true,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
subgraph cluster_0 {
  graph [ label = "Chunk #0", labeljust = "l", color = "blue" ];
  "project-common";
  "project-peer";
}

subgraph cluster_1 {
  graph [ label = "Chunk #1", labeljust = "l", color = "blue" ];
  "project-dev";
  "project-optional";
  "project-prod";
}

  "project-dev";
  "project-common";
  "project-optional";
  "project-prod";
  "project-dev" -> "project-common";
  "project-optional" -> "project-common";
  "project-prod" -> "project-common";
}
`)

    expect(stripAnsi(output)).not.toContain(`digraph G {
    "project-peer" -> "project-common";
  }
  `)
  })
})

describe('transitive dependencies', () => {
  beforeAll(() => {
    preparePackages([
      {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'project-2': 'workspace:1.0.0',
        },
      },
      {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'project-3': 'workspace:1.0.0',
        },
      },
      {
        name: 'project-3',
        version: '1.0.0',
      },
    ])
  })

  test('"graph" should find all dependencies', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'auto',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
  "project-1";
  "project-2";
  "project-3";
  "project-1" -> "project-2";
  "project-2" -> "project-3";
}
`)
  })

  test('"chunks" should give clusters', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'auto',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
      chunks: true,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
subgraph cluster_0 {
  graph [ label = "Chunk #0", labeljust = "l", color = "blue" ];
  "project-3";
}

subgraph cluster_1 {
  graph [ label = "Chunk #1", labeljust = "l", color = "blue" ];
  "project-2";
}

subgraph cluster_2 {
  graph [ label = "Chunk #2", labeljust = "l", color = "blue" ];
  "project-1";
}

  "project-2";
  "project-3";
  "project-1";
  "project-2" -> "project-3";
  "project-1" -> "project-2";
}
`)
  })
})

describe('circular dependencies', () => {
  beforeAll(() => {
    preparePackages([
      {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'project-2': 'workspace:1.0.0',
        },
      },
      {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'project-1': 'workspace:1.0.0',
        },
      },
    ])
  })

  test('"graph" should find all dependencies', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'auto',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
  "project-1";
  "project-2";
  "project-1" -> "project-2";
  "project-2" -> "project-1";
}
`)
  })

  test('"chunks" should give clusters', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'auto',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
      chunks: true,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
subgraph cluster_0 {
  graph [ label = "Chunk #0", labeljust = "l", color = "blue" ];
  "project-1";
}

subgraph cluster_1 {
  graph [ label = "Chunk #1", labeljust = "l", color = "blue" ];
  "project-2";
}

  "project-1";
  "project-2";
  "project-1" -> "project-2";
  "project-2" -> "project-1";
}
`)
  })
})

describe('color option', () => {
  beforeAll(() => {
    prepare({
      name: 'project-1',
      version: '1.0.0',
    })
  })

  test('"always" produces color', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'always',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
      chunks: true,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
subgraph cluster_0 {
  graph [ label = "Chunk #0", labeljust = "l", color = "blue" ];
  ".";
}

}
`)
  })

  test('"never" does not produce color', async () => {
    const { selectedProjectsGraph } = await readProjects(process.cwd(), [])

    const output = await graph.handler({
      color: 'never',
      recursive: true,
      workspaceDir: process.cwd(),
      selectedProjectsGraph,
      chunks: true,
    })

    expect(stripAnsi(output)).toBe(`digraph G {
subgraph cluster_0 {
  graph [ label = "Chunk #0", labeljust = "l" ];
  ".";
}

}
`)
  })
})
