import path from 'path'
import { runNpm } from '@pnpm/run-npm'
import * as renderHelpModule from 'render-help'
import { type Config, types as allTypes, getWorkspaceConcurrency } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { type ProjectRootDir, type Project, type ProjectRootDirRealPath } from '@pnpm/types'
import { sortPackages } from '@pnpm/sort-packages'
import pLimit from 'p-limit'
import { pick } from 'ramda'

const renderHelp = renderHelpModule as any // eslint-disable-line @typescript-eslint/no-explicit-any

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'npm-path',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    recursive: Boolean,
  }
}

export const commandNames = ['version']

export function help (): string {
  return renderHelp({
    description: 'Bump a package version',
    usages: ['pnpm version <new-version>'],
  })
}

export type VersionOpts = Pick<Config,
    | 'npmPath'
    | 'dir'
    | 'extraEnv'
    | 'configDir'
> & Required<Pick<Config, 'selectedProjectsGraph'>> & {
  recursive?: boolean
  reverse?: boolean
  sort?: boolean
  workspaceConcurrency?: number
}

export async function handler (
  opts: VersionOpts,
  params: string[]
): Promise<void> {
  let chunks!: ProjectRootDir[][]
  if (opts.recursive) {
    chunks = opts.sort
      ? sortPackages(opts.selectedProjectsGraph)
      : [(Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]).sort()]
    if (opts.reverse) {
      chunks = chunks.reverse()
    }
  } else {
    chunks = [[(opts.dir ?? process.cwd()) as ProjectRootDir]]
    const projectInfo = await tryReadProjectManifest(opts.dir)
    if (projectInfo.manifest != null) {
      opts.selectedProjectsGraph = {
        [opts.dir]: {
          dependencies: [],
          package: {
            manifest: projectInfo.manifest,
            rootDir: opts.dir as ProjectRootDir,
            rootDirRealPath: opts.dir as ProjectRootDirRealPath,
          } as Project,
        },
      }
    }
  }

  if (!opts.selectedProjectsGraph) {
    throw new PnpmError('RECURSIVE_VERSION_NO_PACKAGE', 'No package found in this workspace')
  }

  const limitRun = pLimit(getWorkspaceConcurrency(opts.workspaceConcurrency))
  const userConfigPath = opts.configDir ? path.join(opts.configDir, 'rc') : undefined
  let exitCode = 0

  for (const chunk of chunks) {
    // eslint-disable-next-line no-await-in-loop
    await Promise.all(chunk.map(async (prefix) =>
      limitRun(async () => {
        try {
          const { status } = runNpm(opts.npmPath, ['version', ...params], {
            cwd: prefix,
            env: opts.extraEnv,
            userConfigPath,
          })
          if (status !== 0) {
            exitCode = status ?? 1
          }
        } catch (err: any) { // eslint-disable-line
          if (!opts.recursive && typeof err.exitCode === 'number') {
            exitCode = err.exitCode
            return
          }
          throw err
        }
      })
    ))
  }

  if (exitCode !== 0) {
    process.exit(exitCode)
  }
}
