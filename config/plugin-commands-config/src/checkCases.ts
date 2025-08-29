/**
 * Check if a name is strictly kebab-case.
 *
 * "Strictly kebab-case" means that the name is kebab-case and has at least 2 words.
 */
export function isStrictlyKebabCase (name: string): boolean {
  const segments = name.split('-')
  if (segments.length < 2) return false
  return segments.every(segment => /^[a-z][a-z0-9]*$/.test(segment))
}

/**
 * Check if a name is camelCase.
 */
export function isCamelCase (name: string): boolean {
  return /^[a-z][a-zA-Z0-9]*$/.test(name)
}
