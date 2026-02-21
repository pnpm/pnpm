import { docsUrl } from '@pnpm/cli-utils'
import { type Config } from '@pnpm/config'
import { findWorkspacePackagesNoCheck, Project } from  '@pnpm/workspace.find-packages'
import { findWorkspaceDir } from  '@pnpm/find-workspace-dir'
import { PnpmError } from '@pnpm/error'
import enquirer from 'enquirer'
import chalk from 'chalk'
import renderHelp from 'render-help'
import { readWorkspaceManifest, WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    interactive: Boolean,
  }
}

export const commandNames = ['migrate']

export const description = 'Migrates dependencies to using catalogs'

export function help (): string {
  return renderHelp({
    description,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Interactively migrate to catalogs',
            name: '--interactive',
            shortAlias: '-i',
          },
        ],
      },
    ],
    url: docsUrl('catalogs'),
    usages: ['pnpm catalog migrate'],
  })
}

export type CatalogMigrateCommandOptions = Pick<Config, 'cliOptions'> & {
  interactive?: boolean
}

export async function handler (opts: CatalogMigrateCommandOptions, params: string[]): Promise<string> {
  if (opts.interactive) {
    return interactiveMigrate(opts, params)
  }
  return migrate(opts, params)
}

const dependencyTypes = [
  'dependencies',
  'devDependencies',
  'peerDependencies',
  'optionalDependencies',
] as const satisfies (keyof Project['manifest'])[]

async function migrate (opts: CatalogMigrateCommandOptions, params: string[]): Promise<string> {
  const workspace = await getWorkspaceInfo(opts, params)

  if (!workspace) {
    throw new PnpmError('WORKSPACE_INFO_MISSING', 'Could not retrieve workspace information')
  }

  const { manifest, dir, packages, dependencies } = workspace

  if (!manifest.catalog) {
    manifest.catalog = {}
  }

  let hasChanges = false

  await Promise.all(packages.map(async (pkg) => {
    for (const dependencyType of dependencyTypes) {
      const deps = pkg.manifest[dependencyType]
      if (!deps) continue

      for (const depName of Object.keys(deps)) {
        const versions = dependencies[depName]
        if (versions && versions.size > 1) {
          continue
        }
        deps[depName] = 'catalog:'
        manifest.catalog![depName] = Array.from(versions)[0]
        hasChanges = true
      }
    }

    await pkg.writeProjectManifest(pkg.manifest, true)
  }))

  await updateWorkspaceManifest(dir, { updatedCatalogs: { default: manifest.catalog } })

  const dependencyConflicts = Object.entries(dependencies).filter(([_, versions]) => versions.size > 1)

  if (dependencyConflicts.length > 0) {
    console.log(chalk.yellow('The following dependencies have version conflicts and were not migrated to catalogs:') + '\n' + dependencyConflicts.map(([depName, versions]) => `- ${depName}: ${Array.from(versions).join(', ')}`).join('\n'))
  }

  if (!hasChanges) {
    return chalk.green('No dependencies were migrated to catalogs.')
  }

  return chalk.green('Migration completed. Please review the changes and run `pnpm install` to update your lockfile.')
}

type VersionChoice = {
  name: string
  message: string
  version: string | undefined
  value: [string, string] | undefined
  disabled: boolean,
  enabled: boolean
  hint: string
  hasConflicts: boolean
  indent: string
}

async function interactiveMigrate (opts: CatalogMigrateCommandOptions, params: string[]): Promise<string> {
  const workspace = await getWorkspaceInfo(opts, params)

  if (!workspace) {
    throw new PnpmError('WORKSPACE_INFO_MISSING', 'Could not retrieve workspace information')
  }

  const { manifest, dir, packages, dependencies } = workspace

  if (!manifest.catalog) {
    manifest.catalog = {}
  }

  const choices = Object.entries(dependencies).flatMap(([depName, versions]) => {
    if (versions && versions.size > 1) {
      return [
        {
          name: depName,
          message: depName,
          version: undefined,
          disabled: true,
          enabled: false,
          hint: `Conflict: ${Array.from(versions).join(', ')}`,
          hasConflicts: true,
          indent: '',
        },
        ...Array.from(versions).map((version) => ({
          name: `${depName}@${version}`,
          message: depName,
          version,
          value: [depName, version],
          disabled: false,
          enabled: false,
          hint: `Set to version ${version}`,
          hasConflicts: false,
          indent: '  ',
        })),
      ]
    }
    const version = Array.from(versions)[0]
    return [
      {
        name: `${depName}@${version}`,
        message: depName,
        version,
        value: [depName, version],
        disabled: false,
        enabled: false,
        hint: '',
        hasConflicts: false,
        indent: '',
      },
    ]
  }).filter(Boolean) as VersionChoice[]

  const { value: migrationPlan } = await enquirer.prompt({
    type: 'multiselect',
    choices,
    message: 'Choose which dependencies to migrate to catalogs',
    name: 'value',
    pointer: '❯',
    indicator: (state: unknown, choice: VersionChoice) => {
      return ` ${choice.enabled ? '●' : choice.hasConflicts ? chalk.yellow('⚠') : '○'}`
    },
    toggle (this: { choices: VersionChoice[], emit: (event: string, choice: VersionChoice, context: unknown) => void }, choice: VersionChoice, enabled:  boolean) {
      const samePackageChoices = this.choices.filter((c: VersionChoice) => c.message === choice.message && c.name !== choice.name  && !c.hasConflicts)
      if (!enabled && samePackageChoices.length > 0) {
        for (const c of samePackageChoices) {
          c.enabled = false
        }
      }
      if (typeof enabled !== 'boolean') enabled = !choice.enabled
      choice.enabled = enabled
      this.emit('toggle', choice, this)
      return choice
    },
    result (names: unknown) {
      return this.map(names)
    },
    styles: {
      dark: chalk.reset,
      em: chalk.bgBlack.whiteBright,
      success: chalk.reset,
    },
    cancel () {
      process.exit(0)
    },
  } as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any

  if (!migrationPlan || Object.keys(migrationPlan).length === 0) {
    return chalk.green('No dependencies were migrated to catalogs.')
  }

  for (const [depName, version] of Object.values(migrationPlan) as [string, string][]) {
    manifest.catalog[depName] = version
    for (const pkg of packages) {
      for (const dependencyType of dependencyTypes) {
        const deps = pkg.manifest[dependencyType]
        if (!deps) continue

        if (deps[depName]) {
          deps[depName] = 'catalog:'
        }
      }
    }
  }

  await Promise.all(packages.map(async (pkg) => pkg.writeProjectManifest(pkg.manifest, true)))

  await updateWorkspaceManifest(dir, { updatedCatalogs: { default: manifest.catalog } })

  return chalk.green('Migration completed. Please review the changes and run `pnpm install` to update your lockfile.')
}

async function getWorkspaceInfo (_opts: CatalogMigrateCommandOptions, _params: string[]): Promise<{
  manifest: WorkspaceManifest
  dir: string
  packages: Project[]
  dependencies: Record<string, Set<string>>
} | undefined> {
  const workspaceDir = await findWorkspaceDir(process.cwd())

  if (!workspaceDir) {
    throw new PnpmError('WORKSPACE_DIR_MISSING', 'Could not find workspace directory')
  }

  const workspacePackages = await findWorkspacePackagesNoCheck(workspaceDir)
  const workspaceManifest = await readWorkspaceManifest(workspaceDir) ?? { packages: workspacePackages.map((pkg) => pkg.manifest.name).filter(Boolean) as string[] } satisfies WorkspaceManifest

  const workspaceDependencies: Record<string, Set<string>> = {}

  for (const workspacePackage of workspacePackages) {
    if (!workspacePackage) {
      return undefined
    }

    for (const dependencyType of dependencyTypes satisfies (keyof Project['manifest'])[]) {
      if (!workspacePackage.manifest[dependencyType]) {
        continue
      }

      for (const [dependency,  version] of Object.entries(workspacePackage.manifest[dependencyType])) {
        if (version.startsWith('catalog:')) {
          continue
        }
        if (!workspaceDependencies[dependency]) {
          workspaceDependencies[dependency] = new Set([version])
          continue
        }

        workspaceDependencies[dependency].add(version)
      }
    }
  }

  return { manifest: workspaceManifest, dir: workspaceDir, packages: workspacePackages, dependencies: workspaceDependencies }
}