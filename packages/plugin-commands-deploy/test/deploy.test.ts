import { deploy } from '@pnpm/plugin-commands-deploy'
import { preparePackages } from '@pnpm/prepare'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { DEFAULT_OPTS } from './utils'

test('deploy', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'project-2': 'workspace:*',
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'project-3': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, [])
})
