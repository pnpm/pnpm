export function ignoreDependenciesWithSelectorPattern (selectorPattern: string[], ignoredDependencies: string[]): string[] {
  return [...ignoredDependencies.map(depName => `!${depName}`), ...selectorPattern]
}