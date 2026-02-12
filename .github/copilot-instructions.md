# GitHub Copilot Instructions for pnpm Repository

This document provides guidance for AI coding assistants (GitHub Copilot, Claude, etc.) working on the pnpm codebase. These guidelines help maintain code quality and avoid common anti-patterns.

## Testing Best Practices

### ❌ Anti-Pattern 1: Using try/catch in Tests

**DO NOT** use try/catch blocks to test error scenarios in Jest tests.

```typescript
// ❌ BAD: This pattern is error-prone and can lead to false positives
test('should throw error', async () => {
  try {
    await someAsyncFunction(invalidInput)
    // If function doesn't throw, test silently passes! ⚠️
  } catch (error) {
    if (error instanceof ExpectedError) {
      expect(error.message).toContain('expected')
      expect(error.code).toBe('ERR_CODE')
    }
    // If error is wrong type, assertions never run! ⚠️
  }
})
```

**Problems with this pattern:**
1. **Silent false positives**: If the function doesn't throw an error, the test passes without running any assertions
2. **Conditional assertions**: If the error is not the expected type, assertions inside the catch block are skipped silently
3. **Poor readability**: Nested code with implicit behavior is harder to understand and maintain
4. **Race conditions**: Multiple awaits of the same promise can lead to unexpected behavior

### ❌ Anti-Pattern 2: Conditional Assertions with instanceof

**DO NOT** wrap assertions in `if` statements based on error type checks.

```typescript
// ❌ BAD: Assertions may never execute
if (error instanceof SomeError) {
  expect(error.someProp).toBe(expectedValue)
  expect(error.anotherProp).toBe(otherValue)
}
// If error is not SomeError, test passes without running assertions! ⚠️
```

**Problems with this pattern:**
1. **Silently skipped assertions**: If the condition is false, assertions never run but test still passes
2. **False confidence**: You think you're testing something, but you're not
3. **Maintenance burden**: Easy to forget to add explicit failure cases

### ✅ Correct Pattern: Jest's Promise Rejection Matchers

**ALWAYS** use Jest's built-in promise rejection matchers with a stored promise variable.

```typescript
// ✅ GOOD: Explicit, reliable, and follows Jest best practices
test('should throw error with correct properties', async () => {
  // Step 1: Call the async function WITHOUT await, store the promise
  const promise = someAsyncFunction(invalidInput)
  
  // Step 2: Use Jest matchers - each one verifies the promise rejects
  await expect(promise).rejects.toBeInstanceOf(ExpectedError)
  await expect(promise).rejects.toHaveProperty('message', expect.stringContaining('expected'))
  await expect(promise).rejects.toHaveProperty('code', 'ERR_CODE')
  await expect(promise).rejects.toHaveProperty(['nested', 'property'], 'value')
  
  // All assertions are guaranteed to run, or the test fails! ✅
})
```

**Why this pattern is better:**
1. **Jest ensures an error is thrown**: The test fails if the promise doesn't reject
2. **All assertions are explicit**: Each `await expect()` must pass or the test fails
3. **Better error messages**: Jest provides clear failure messages for each assertion
4. **More readable**: Intent is clear, no hidden behavior
5. **Prevents false positives**: Cannot accidentally pass without testing

### ✅ Pattern Details: The `promise` Variable

**Key principle**: Call the async function ONCE, store the result, use it multiple times.

```typescript
// ✅ Store the promise in a variable
const promise = asyncFunction(params)

// ✅ Run multiple assertions on the same promise
await expect(promise).rejects.toBeInstanceOf(CustomError)
await expect(promise).rejects.toHaveProperty('statusCode', 404)
await expect(promise).rejects.toHaveProperty(['body', 'message'], 'Not Found')

// Each await expect() re-evaluates the promise rejection
// Jest handles promise caching internally
```

### Additional Testing Guidelines

#### Keep Test Code Simple

- **Inline literal values**: Don't create variables for simple test data unless reused
- **Use direct assertions**: `expect(url).toBe(exactString)` is better than `expect(url).toContain(part1) && expect(url).toContain(part2)`
- **Avoid unnecessary abstractions**: Don't create helper functions for one-time use

#### Type Assertions in Tests

When mocking functions, use type assertions to maintain type safety:

```typescript
// ✅ GOOD: Type assertion for mock functions
const mockFetch = jest.fn() as jest.MockedFunction<typeof fetch>
```

## Summary

**Golden Rules for Testing:**

1. ✅ **DO** use `const promise = asyncFunc()` then `await expect(promise).rejects.*`
2. ✅ **DO** keep test code simple and explicit
3. ❌ **DON'T** use try/catch to test error scenarios
4. ❌ **DON'T** wrap assertions in `if` statements

Following these patterns ensures reliable, maintainable tests that catch real bugs and don't create false confidence through passing tests that don't actually verify behavior.
