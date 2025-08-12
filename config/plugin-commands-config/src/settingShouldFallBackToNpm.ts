
export function settingShouldFallBackToNpm (key: string): boolean {
  return (
    ['registry', '_auth', '_authToken', 'username', '_password'].includes(key) ||
    key[0] === '@' ||
    key.startsWith('//')
  );
}
