import type { LicensesConfig, ProjectManifest } from '@pnpm/types'

export interface IncludeFlags {
  dev?: boolean
  production?: boolean
  optional?: boolean
}

export function resolveInclude (
  environment: string,
  opts?: IncludeFlags
): { dependencies: boolean, devDependencies: boolean, optionalDependencies: boolean } {
  if (environment === 'prod') {
    return {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: opts?.optional !== false,
    }
  }
  if (environment === 'dev') {
    return {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    }
  }
  return {
    dependencies: opts?.production !== false,
    devDependencies: opts?.dev !== false,
    optionalDependencies: opts?.optional !== false,
  }
}

export function collectDirectDeps (
  manifest: ProjectManifest,
  selectedProjectsGraph?: Record<string, { package: { manifest: ProjectManifest } }>
): Set<string> {
  const manifests: ProjectManifest[] = [manifest]
  if (selectedProjectsGraph) {
    for (const project of Object.values(selectedProjectsGraph)) {
      manifests.push(project.package.manifest)
    }
  }
  const deps = new Set<string>()
  for (const m of manifests) {
    for (const name of Object.keys(m.dependencies ?? {})) deps.add(name)
    for (const name of Object.keys(m.devDependencies ?? {})) deps.add(name)
    for (const name of Object.keys(m.optionalDependencies ?? {})) deps.add(name)
  }
  return deps
}

export function shouldRunLicenseCheck (licenses?: LicensesConfig | null): boolean {
  const mode = licenses?.mode
  return mode === 'strict' || mode === 'loose'
}
