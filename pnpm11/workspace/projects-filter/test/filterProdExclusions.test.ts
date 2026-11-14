import { describe, expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
import { filterProjects, orderWorkspaceFilters } from '@pnpm/workspace.projects-filter'
import type { BaseProject } from '@pnpm/workspace.projects-graph'

function makeProject (
  dir: string,
  name: string,
  deps: { prod?: Record<string, string>, dev?: Record<string, string> } = {}
): BaseProject {
  return {
    rootDir: dir as ProjectRootDir,
    manifest: {
      name,
      version: '1.0.0',
      dependencies: deps.prod,
      devDependencies: deps.dev,
    },
  }
}

describe('filterProjects combining --filter and --filter-prod with exclusions', () => {
  test('--filter-prod inclusion followed by regular exclusion excludes target without seeding workspace', async () => {
    const projects: BaseProject[] = [
      makeProject('/ws', 'ws-root'),
      makeProject('/ws/packages/app', 'app', {
        prod: { 'lib-a': 'workspace:*' },
        dev: { 'lib-dev': 'workspace:*' },
      }),
      makeProject('/ws/packages/lib-a', 'lib-a'),
      makeProject('/ws/packages/lib-dev', 'lib-dev'),
      makeProject('/ws/packages/unrelated', 'unrelated'),
    ]

    const result = await filterProjects(projects, [
      { filter: 'app...', followProdDepsOnly: true },
      { filter: '!lib-a', followProdDepsOnly: false },
    ], { prefix: '/ws', workspaceDir: '/ws', useGlobDirFiltering: true })

    expect(Object.keys(result.selectedProjectsGraph)).toStrictEqual(['/ws/packages/app'])
    expect(result.prodOnlySelectedProjectDirs).toStrictEqual(['/ws/packages/app'])
    expect(result.selectedProjectsGraph['/ws/packages/app' as ProjectRootDir].dependencies).toStrictEqual(['/ws/packages/lib-a'])
  })

  test('regular inclusion followed by --filter-prod exclusion subtracts only prod closure', async () => {
    const projects: BaseProject[] = [
      makeProject('/ws', 'ws-root'),
      makeProject('/ws/app', 'app', { prod: { 'lib-a': 'workspace:*' } }),
      makeProject('/ws/lib-a', 'lib-a', {
        prod: { 'prod-leaf': 'workspace:*' },
        dev: { 'dev-leaf': 'workspace:*' },
      }),
      makeProject('/ws/prod-leaf', 'prod-leaf'),
      makeProject('/ws/dev-leaf', 'dev-leaf'),
      makeProject('/ws/unrelated', 'unrelated'),
    ]

    const result = await filterProjects(projects, [
      { filter: 'app...', followProdDepsOnly: false },
      { filter: '!lib-a...', followProdDepsOnly: true },
    ], { prefix: '/ws', workspaceDir: '/ws', useGlobDirFiltering: true })

    expect(Object.keys(result.selectedProjectsGraph)).toStrictEqual(['/ws/app', '/ws/dev-leaf'])
    expect(result.prodOnlySelectedProjectDirs).toStrictEqual([])
  })

  test('leading exclusion followed by --filter-prod inclusion re-includes prod closure', async () => {
    const projects: BaseProject[] = [
      makeProject('/ws', 'ws-root'),
      makeProject('/ws/packages/app', 'app', {
        prod: { 'lib-a': 'workspace:*' },
        dev: { 'lib-dev': 'workspace:*' },
      }),
      makeProject('/ws/packages/lib-a', 'lib-a'),
      makeProject('/ws/packages/lib-dev', 'lib-dev'),
      makeProject('/ws/packages/unrelated', 'unrelated'),
    ]

    const result = await filterProjects(projects, [
      { filter: '!{packages/*}', followProdDepsOnly: false },
      { filter: 'app...', followProdDepsOnly: true },
    ], { prefix: '/ws', workspaceDir: '/ws', useGlobDirFiltering: true })

    expect(Object.keys(result.selectedProjectsGraph)).toStrictEqual([
      '/ws/packages/app',
      '/ws/packages/lib-a',
      '/ws',
    ])
    expect(result.prodOnlySelectedProjectDirs).toStrictEqual([
      '/ws/packages/app',
      '/ws/packages/lib-a',
    ])
  })
})

describe('orderWorkspaceFilters', () => {
  test('preserves interleaved CLI order from cookedArgv', () => {
    expect(orderWorkspaceFilters({
      filter: ['!lib-a'],
      filterProd: ['app...'],
      cliOptions: { filter: ['!lib-a'], 'filter-prod': ['app...'] },
      cookedArgv: ['--filter-prod', 'app...', '--filter', '!lib-a', 'run', 'check'],
    })).toStrictEqual([
      { filter: 'app...', followProdDepsOnly: true },
      { filter: '!lib-a', followProdDepsOnly: false },
    ])

    expect(orderWorkspaceFilters({
      filter: ['!{packages/*}'],
      filterProd: ['app...'],
      cliOptions: { filter: ['!{packages/*}'], 'filter-prod': ['app...'] },
      cookedArgv: ['--filter', '!{packages/*}', '--filter-prod', 'app...', 'run', 'check'],
    })).toStrictEqual([
      { filter: '!{packages/*}', followProdDepsOnly: false },
      { filter: 'app...', followProdDepsOnly: true },
    ])
  })

  test('preserves CLI order for abbreviated --filter-prod flags and inline = values', () => {
    expect(orderWorkspaceFilters({
      filter: ['app...'],
      filterProd: ['!lib-a'],
      cliOptions: { filter: ['app...'], 'filter-prod': ['!lib-a'] },
      cookedArgv: ['--filter-p', '!lib-a', '--filter', 'app...', 'run', 'check'],
    })).toStrictEqual([
      { filter: '!lib-a', followProdDepsOnly: true },
      { filter: 'app...', followProdDepsOnly: false },
    ])

    expect(orderWorkspaceFilters({
      filter: ['!lib-a'],
      filterProd: ['app...'],
      cliOptions: { filter: ['!lib-a'], 'filter-prod': ['app...'] },
      cookedArgv: ['--filter-prod=app...', '--filter=!lib-a', 'run', 'check'],
    })).toStrictEqual([
      { filter: 'app...', followProdDepsOnly: true },
      { filter: '!lib-a', followProdDepsOnly: false },
    ])

    expect(orderWorkspaceFilters({
      filter: ['!lib-a'],
      filterProd: ['app...'],
      cliOptions: { filter: ['!lib-a'], 'filter-prod': ['app...'] },
      cookedArgv: ['--filter=!lib-a', '--filter-prod=app...', 'run', 'check'],
    })).toStrictEqual([
      { filter: '!lib-a', followProdDepsOnly: false },
      { filter: 'app...', followProdDepsOnly: true },
    ])
  })

  test('preserves CLI order when nopt trims surrounding whitespace from cookedArgv tokens', () => {
    expect(orderWorkspaceFilters({
      filter: ['!lib-a'],
      filterProd: ['app...'],
      cliOptions: { filter: ['!lib-a'], 'filter-prod': ['app...'] },
      cookedArgv: ['--filter', ' !lib-a ', '--filter-prod', ' app... ', 'run', 'check'],
    })).toStrictEqual([
      { filter: '!lib-a', followProdDepsOnly: false },
      { filter: 'app...', followProdDepsOnly: true },
    ])
  })

  test('places positive filterProd before leading negated filter when CLI order is unavailable', () => {
    expect(orderWorkspaceFilters({
      filter: ['!lib-a'],
      filterProd: ['app...'],
    })).toStrictEqual([
      { filter: 'app...', followProdDepsOnly: true },
      { filter: '!lib-a', followProdDepsOnly: false },
    ])
  })
})
