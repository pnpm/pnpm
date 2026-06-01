export function interactivePromptPageSize (): number {
  const availableRows = process.stdout.rows
  return availableRows == null ? 7 : Math.max(7, availableRows - 6)
}
