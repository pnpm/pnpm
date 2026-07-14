export function resolve (specifier, context, nextResolve) {
  return nextResolve(specifier, context)
}
