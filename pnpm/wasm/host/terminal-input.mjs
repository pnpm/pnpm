const activeInputs = new WeakSet()

export function claimTerminalInput (input) {
  if (activeInputs.has(input)) throw new Error('A terminal prompt is already active')
  activeInputs.add(input)
  return () => activeInputs.delete(input)
}
