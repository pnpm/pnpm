import fs from 'fs/promises'
import path from 'path'
import writeYamlFile from 'write-yaml-file'
import { execPnpm } from './utils'
import {
  preparePackages,
} from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'

test.each([
  { message: '--filter should include devDependencies', filter: '--filter', expected: new Set(['project-1', 'project-3', 'project-4']) },
  { message: '--filter-prod should not include devDependencies', filter: '--filter-prod', expected: new Set(['project-1', 'project-3']) },
])('$message', async ({ filter, expected }) => {
  // Using backticks in scripts for better readability. Otherwise single quotes need to be escaped.
  /* eslint-disable @typescript-eslint/quotes */
  const projects: Array<ProjectManifest & { name: string }> = [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      scripts: {
        test: `node -e "require('fs').writeFileSync('./output.txt', '')"`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {},
      scripts: {
        test: `node -e "require('fs').writeFileSync('./output.txt', '')"`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0' },
      scripts: {
        test: `node -e "require('fs').writeFileSync('./output.txt', '')"`,
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      dependencies: {},
      devDependencies: { 'project-3': '1.0.0' },
      scripts: {
        test: `node -e "require('fs').writeFileSync('./output.txt', '')"`,
      },
    },
  ]
  preparePackages(projects)

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['install'])

  // Ensure none of these files exist before the build script runs.
  await Promise.all(projects.map(async project => {
    await expect(fs.access(path.resolve(project.name, 'output.txt'))).rejects.toThrow()
  }))

  await execPnpm(['recursive', 'test', filter, '...project-3'])

  await Promise.all(projects.map(async project => {
    if (expected.has(project.name)) {
      await expect(fs.access(path.resolve(project.name, 'output.txt'))).resolves.not.toThrow()
    } else {
      await expect(fs.access(path.resolve(project.name, 'output.txt'))).rejects.toThrow()
    }
  }))
})
