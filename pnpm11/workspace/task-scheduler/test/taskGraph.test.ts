import { expect, test } from '@jest/globals'
import type { PackageScripts, ProjectRootDir } from '@pnpm/types'
import {
  buildTaskGraph,
  isSerialTaskGraph,
  renderTaskGraphDryRun,
  resumeTaskGraphFrom,
  reverseTaskGraph,
  sequenceTasks,
  type TaskGraph,
  taskGraphToJson,
  taskKey,
} from '@pnpm/workspace.task-scheduler'

const WORKSPACE_DIR = '/workspace'

test('an unconfigured task depends on the same task in the workspace dependencies', () => {
  const graph = buildGraph({
    a: { dependencies: ['b'], scripts: ['build'] },
    b: { scripts: ['build'] },
  }, 'build')

  expect([...graph.keys()].sort()).toStrictEqual([taskKey(dir('a'), 'build'), taskKey(dir('b'), 'build')].sort())
  expect(graph.get(taskKey(dir('a'), 'build'))!.dependencies).toStrictEqual([taskKey(dir('b'), 'build')])
  expect(graph.get(taskKey(dir('b'), 'build'))!.dependencies).toStrictEqual([])
  expect(graph.get(taskKey(dir('a'), 'build'))!.requested).toBe(true)
})

test('a same-project dependsOn entry pulls the named task into the graph', () => {
  const graph = buildGraph({
    a: { dependencies: ['b'], scripts: ['build', 'test'] },
    b: { scripts: ['build', 'test'] },
  }, 'test', {
    build: { dependsOn: ['^build'] },
    test: { dependsOn: ['build'] },
  })

  expect([...graph.keys()].sort()).toStrictEqual([
    taskKey(dir('a'), 'build'),
    taskKey(dir('a'), 'test'),
    taskKey(dir('b'), 'build'),
    taskKey(dir('b'), 'test'),
  ].sort())
  expect(graph.get(taskKey(dir('a'), 'test'))!.dependencies).toStrictEqual([taskKey(dir('a'), 'build')])
  expect(graph.get(taskKey(dir('a'), 'build'))!.dependencies).toStrictEqual([taskKey(dir('b'), 'build')])
  expect(graph.get(taskKey(dir('a'), 'build'))!.requested).toBe(false)
  expect(graph.get(taskKey(dir('a'), 'test'))!.requested).toBe(true)
})

test('an explicitly empty dependsOn means the task depends on nothing', () => {
  const graph = buildGraph({
    a: { dependencies: ['b'], scripts: ['lint'] },
    b: { scripts: ['lint'] },
  }, 'lint', {
    lint: {},
  })

  expect(graph.get(taskKey(dir('a'), 'lint'))!.dependencies).toStrictEqual([])
})

test('a project without the script becomes a pass-through node that keeps the chain', () => {
  const graph = buildGraph({
    a: { dependencies: ['b'], scripts: ['build'] },
    b: { dependencies: ['c'] },
    c: { scripts: ['build'] },
  }, 'build')

  const passThrough = graph.get(taskKey(dir('b'), 'build'))!
  expect(passThrough.scripts).toStrictEqual([])
  expect(passThrough.dependencies).toStrictEqual([taskKey(dir('c'), 'build')])
  expect(graph.get(taskKey(dir('a'), 'build'))!.dependencies).toStrictEqual([taskKey(dir('b'), 'build')])

  const groups = sequenceTasks(graph, { workspaceDir: WORKSPACE_DIR })
  expect(groups).toStrictEqual([
    [taskKey(dir('c'), 'build')],
    [taskKey(dir('b'), 'build')],
    [taskKey(dir('a'), 'build')],
  ])
})

test('a task cycle is an error naming the participating tasks', () => {
  expect(() => {
    sequenceTasks(buildGraph({
      a: { dependencies: ['b'], scripts: ['build', 'test'] },
      b: { dependencies: ['a'], scripts: ['build'] },
    }, 'build'), { workspaceDir: WORKSPACE_DIR })
  }).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_TASK_CYCLE',
    message: expect.stringMatching(/a#build.*b#build.*a#build|b#build.*a#build.*b#build/),
  }))
})

test('a task depending on itself is an error', () => {
  expect(() => {
    sequenceTasks(buildGraph({
      a: { scripts: ['build'] },
    }, 'build', {
      build: { dependsOn: ['build'] },
    }), { workspaceDir: WORKSPACE_DIR })
  }).toThrow(expect.objectContaining({ code: 'ERR_PNPM_TASK_CYCLE' }))
})

test('a cycle among unselected projects cannot fail the run', () => {
  // Only `a` is selected; the b <-> c cycle exists in the workspace but not
  // in this invocation's graph.
  const graph = buildGraph({
    a: { scripts: ['build'] },
  }, 'build')
  expect(() => sequenceTasks(graph, { workspaceDir: WORKSPACE_DIR })).not.toThrow()
})

test('reverseTaskGraph runs dependents before dependencies', () => {
  const graph = buildGraph({
    a: { dependencies: ['b'], scripts: ['build'] },
    b: { scripts: ['build'] },
  }, 'build')

  const reversed = reverseTaskGraph(graph)
  expect(reversed.get(taskKey(dir('a'), 'build'))!.dependencies).toStrictEqual([])
  expect(reversed.get(taskKey(dir('b'), 'build'))!.dependencies).toStrictEqual([taskKey(dir('a'), 'build')])
})

test('resumeTaskGraphFrom drops only the anchor\'s transitive dependencies', () => {
  const graph = buildGraph({
    a: { scripts: ['build'] },
    b: { dependencies: ['a'], scripts: ['build'] },
    c: { dependencies: ['b'], scripts: ['build'] },
    unrelated: { scripts: ['build'] },
  }, 'build')

  const resumed = resumeTaskGraphFrom(graph, {
    resumeFrom: 'b',
    selectedProjectsGraph: Object.fromEntries(['a', 'b', 'c', 'unrelated'].map((name) => [
      dir(name),
      { dependencies: [], package: { manifest: { name } } },
    ])) as never,
    taskName: 'build',
  })

  expect([...resumed.keys()].sort()).toStrictEqual([
    taskKey(dir('b'), 'build'),
    taskKey(dir('c'), 'build'),
    taskKey(dir('unrelated'), 'build'),
  ].sort())
  // The edge into the dropped dependency is treated as satisfied.
  expect(resumed.get(taskKey(dir('b'), 'build'))!.dependencies).toStrictEqual([])
  expect(resumed.get(taskKey(dir('c'), 'build'))!.dependencies).toStrictEqual([taskKey(dir('b'), 'build')])
})

test('resumeTaskGraphFrom throws when the package is not selected', () => {
  const graph = buildGraph({ a: { scripts: ['build'] } }, 'build')
  expect(() => resumeTaskGraphFrom(graph, {
    resumeFrom: 'missing',
    selectedProjectsGraph: {
      [dir('a')]: { dependencies: [], package: { manifest: { name: 'a' } } },
    } as never,
    taskName: 'build',
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_RESUME_FROM_NOT_FOUND' }))
})

test('isSerialTaskGraph tells a chain from a graph with independent tasks', () => {
  const chain = buildGraph({
    a: { dependencies: ['b'], scripts: ['build'] },
    b: { dependencies: ['c'], scripts: ['build'] },
    c: { scripts: ['build'] },
  }, 'build')
  expect(isSerialTaskGraph(chain, sequenceTasks(chain, { workspaceDir: WORKSPACE_DIR }))).toBe(true)

  const diamond = buildGraph({
    a: { dependencies: ['b', 'c'], scripts: ['build'] },
    b: { dependencies: ['d'], scripts: ['build'] },
    c: { dependencies: ['d'], scripts: ['build'] },
    d: { scripts: ['build'] },
  }, 'build')
  expect(isSerialTaskGraph(diamond, sequenceTasks(diamond, { workspaceDir: WORKSPACE_DIR }))).toBe(false)
})

test('isSerialTaskGraph sees through pass-through tasks', () => {
  const graph = buildGraph({
    a: { dependencies: ['b'], scripts: ['build'] },
    b: { dependencies: ['c'] },
    c: { scripts: ['build'] },
  }, 'build')
  expect(isSerialTaskGraph(graph, sequenceTasks(graph, { workspaceDir: WORKSPACE_DIR }))).toBe(true)
})

test('taskGraphToJson emits sorted nodes and edges with a missing-script flag', () => {
  const graph = buildGraph({
    b: { dependencies: ['a'], scripts: ['build'] },
    a: { dependencies: [] },
  }, 'build')

  expect(taskGraphToJson(graph, WORKSPACE_DIR)).toStrictEqual({
    tasks: [
      {
        project: 'a',
        script: 'build',
        missingScript: true,
        dependsOn: [],
      },
      {
        project: 'b',
        script: 'build',
        missingScript: false,
        dependsOn: [{ project: 'a', script: 'build' }],
      },
    ],
  })
})

test('renderTaskGraphDryRun prints one stable linearization', () => {
  const graph = buildGraph({
    c: { dependencies: ['b'], scripts: ['build'] },
    b: { dependencies: ['a'] },
    a: { scripts: ['build'] },
  }, 'build')

  expect(renderTaskGraphDryRun(graph, sequenceTasks(graph, { workspaceDir: WORKSPACE_DIR }), WORKSPACE_DIR)).toBe([
    'a#build',
    'b#build (skipped: no such script)',
    'c#build',
  ].join('\n'))
})

test('a script named like an Object prototype member gets the default dependsOn', () => {
  for (const name of ['constructor', 'toString', '__proto__']) {
    const graph = buildGraph({
      a: { dependencies: ['b'], scripts: [name] },
      b: { scripts: [name] },
    }, name, {
      build: { dependsOn: ['^build'] },
    })
    expect(graph.get(taskKey(dir('a'), name))!.dependencies).toStrictEqual([taskKey(dir('b'), name)])
  }
})

test('ignored cycles are downgraded and their edges dropped', () => {
  const graph = buildGraph({
    a: { dependencies: ['b'], scripts: ['build'] },
    b: { dependencies: ['a'], scripts: ['build'] },
    c: { dependencies: ['a'], scripts: ['build'] },
  }, 'build')

  expect(() => sequenceTasks(graph, { workspaceDir: WORKSPACE_DIR, ignoreCycles: true })).not.toThrow()
  // The cycle members lost their mutual edges and may run concurrently; the
  // task outside the cycle still waits for its dependency.
  expect(graph.get(taskKey(dir('a'), 'build'))!.dependencies).toStrictEqual([])
  expect(graph.get(taskKey(dir('b'), 'build'))!.dependencies).toStrictEqual([])
  expect(graph.get(taskKey(dir('c'), 'build'))!.dependencies).toStrictEqual([taskKey(dir('a'), 'build')])
})

function dir (name: string): ProjectRootDir {
  return `${WORKSPACE_DIR}/${name}` as ProjectRootDir
}

interface FakeProject {
  dependencies?: string[]
  scripts?: string[]
}

function buildGraph (
  projects: Record<string, FakeProject>,
  taskName: string,
  tasks?: Record<string, { dependsOn?: string[] }>
): TaskGraph {
  const scriptsByDir = new Map<ProjectRootDir, PackageScripts>(
    Object.entries(projects).map(([name, project]) => [
      dir(name),
      Object.fromEntries((project.scripts ?? []).map((script) => [script, `echo ${script}`])),
    ])
  )
  return buildTaskGraph({
    projectDependencies: new Map(
      Object.entries(projects).map(([name, project]) => [
        dir(name),
        (project.dependencies ?? []).map(dir),
      ])
    ),
    scriptsByProject: (project) => scriptsByDir.get(project)!,
    selectScripts: (scripts, scriptName) => scripts[scriptName] ? [scriptName] : [],
    taskName,
    tasks,
  })
}
