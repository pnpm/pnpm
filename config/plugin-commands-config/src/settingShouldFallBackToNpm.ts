// NOTE: The logic may be duplicated with `isIniConfigKey` from `@pnpm/config`,
//       but we have not the time to refactor it right now.
// TODO: Refactor it when we have the time.
export function settingShouldFallBackToNpm (key: string): boolean {
  return (
    ['registry', '_auth', '_authToken', 'username', '_password'].includes(key) ||
    key[0] === '@' ||
    key.startsWith('//')
  )
}
