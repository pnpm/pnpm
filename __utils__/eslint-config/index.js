import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import stylistic from '@stylistic/eslint-plugin'
import * as importX from 'eslint-plugin-import-x'
import n from 'eslint-plugin-n'
import promise from 'eslint-plugin-promise'
import noDupeConditions from './no-dupe-conditions.js'
import noObjectMethodsOnMap from './no-object-methods-on-map.js'
import jestPlugin from 'eslint-plugin-jest'

export default tseslint.config(
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    files: ['**/*.ts'],

    linterOptions: {
      reportUnusedDisableDirectives: true,
    },

    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
      parserOptions: {
        project: './tsconfig.lint.json',
      },
    },

    plugins: {
      '@stylistic': stylistic,
      'import-x': importX,
      n,
      promise,
      conditions: {
        rules: {
          'no-dupe-conditions': noDupeConditions,
          'no-object-methods-on-map': noObjectMethodsOnMap,
        },
      },
      jest: jestPlugin,
    },

    rules: {
      // Import rules (migrated from eslint-plugin-import)
      'import-x/extensions': ['error', 'always', { ignorePackages: true }],
      'import-x/no-extraneous-dependencies': ['error', {
        devDependencies: ['**/pnpm/src/**', '**/test/**', '**/src/**/*.test.ts'],
      }],
      'import-x/no-default-export': 'error',

      // Stylistic rules (migrated from @typescript-eslint)
      '@stylistic/indent': ['error', 2, {
        FunctionDeclaration: { parameters: 'first' },
        FunctionExpression: { parameters: 'first' },
      }],
      '@stylistic/quotes': ['error', 'single', { avoidEscape: true }],
      '@stylistic/comma-dangle': ['error', {
        arrays: 'always-multiline',
        exports: 'always-multiline',
        functions: 'never',
        imports: 'always-multiline',
        objects: 'always-multiline',
      }],
      '@stylistic/type-annotation-spacing': 'error',
      '@stylistic/brace-style': ['error', '1tbs'],
      '@stylistic/space-before-function-paren': ['error', 'always'],

      // TypeScript rules
      '@typescript-eslint/consistent-indexed-object-style': 'off',
      '@typescript-eslint/prefer-reduce-type-parameter': 'off',
      '@typescript-eslint/naming-convention': [
        'error',
        {
          selector: 'default',
          format: ['camelCase', 'PascalCase', 'UPPER_CASE'],
          leadingUnderscore: 'allow',
          trailingUnderscore: 'allow',
        },
        {
          selector: 'variable',
          format: ['camelCase', 'PascalCase', 'UPPER_CASE'],
          leadingUnderscore: 'allow',
          trailingUnderscore: 'allow',
        },
        {
          selector: 'variable',
          modifiers: ['unused'],
          format: ['camelCase', 'PascalCase', 'UPPER_CASE'],
          leadingUnderscore: 'allow',
        },
        {
          selector: 'memberLike',
          modifiers: ['private'],
          format: ['camelCase'],
          leadingUnderscore: 'allow',
        },
        {
          selector: 'typeLike',
          format: ['PascalCase'],
        },
        {
          selector: 'memberLike',
          format: null,
        },
      ],
      '@typescript-eslint/explicit-function-return-type': 'off',
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/return-await': 'off',
      '@typescript-eslint/no-require-imports': 'error',
      '@typescript-eslint/no-unused-expressions': 'error',
      '@typescript-eslint/no-use-before-define': ['error', { functions: false, classes: false, typedefs: false, variables: false }],
      '@typescript-eslint/triple-slash-reference': 'off',
      '@typescript-eslint/no-non-null-assertion': 'off',
      '@typescript-eslint/consistent-type-assertions': 'off',
      // The typescript-eslint docs recommend disabling this rule if
      // verbatimModuleSyntax is enabled.
      // https://typescript-eslint.io/rules/consistent-type-imports/
      '@typescript-eslint/consistent-type-imports': 'off',
      '@typescript-eslint/strict-boolean-expressions': 'off',
      '@typescript-eslint/no-base-to-string': 'off',
      '@typescript-eslint/no-dynamic-delete': 'off',
      '@typescript-eslint/promise-function-async': 'off',
      '@typescript-eslint/no-misused-promises': 'off',
      '@typescript-eslint/dot-notation': 'off',
      '@typescript-eslint/no-unnecessary-type-assertion': 'off',
      '@typescript-eslint/ban-ts-comment': 'off',
      '@typescript-eslint/explicit-module-boundary-types': 'error',
      '@typescript-eslint/only-throw-error': 'off',
      '@typescript-eslint/no-confusing-void-expression': 'off',
      '@typescript-eslint/restrict-template-expressions': 'off',
      '@typescript-eslint/no-unnecessary-condition': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-call': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-return': 'off',
      '@typescript-eslint/require-await': 'off',
      '@typescript-eslint/unbound-method': 'off',
      '@typescript-eslint/no-unnecessary-type-parameters': 'off',
      '@typescript-eslint/no-extraneous-class': 'off',
      '@typescript-eslint/no-floating-promises': 'off',
      '@typescript-eslint/prefer-promise-reject-errors': 'off',
      '@typescript-eslint/no-deprecated': 'off',
      '@typescript-eslint/no-unused-vars': ['error', {
        argsIgnorePattern: '^_',
        varsIgnorePattern: '^_',
        caughtErrorsIgnorePattern: '^_',
      }],
      "@typescript-eslint/no-import-type-side-effects": "error",

      // Core ESLint rules
      'no-return-await': 'error',
      'no-await-in-loop': 'error',
      'no-multi-str': 'off',
      'no-mixed-operators': 'off',
      curly: 'off',
      'eol-last': 'off',
      'max-len': 'off',
      'no-multiple-empty-lines': 'error',
      'no-redeclare': 'off', // Handled by @typescript-eslint
      'no-restricted-properties': ['error', {
        property: 'substr',
        message: 'Use .slice instead of .substr.',
      }],
      'no-trailing-spaces': 'error',
      'no-var': 'error',
      'no-lone-blocks': 'off',
      'no-empty': ['error', { allowEmptyCatch: true }],
      'prefer-const': 'off',

      // Custom rules
      'conditions/no-dupe-conditions': 'error',
      'conditions/no-object-methods-on-map': 'error',

      // Jest rules
      'jest/no-standalone-expect': 'off',
      'jest/expect-expect': 'off',
      'jest/no-disabled-tests': 'warn',
      'jest/no-focused-tests': 'error',
      'jest/no-identical-title': 'error',
      'jest/valid-expect': 'error',
      'jest/valid-expect-in-promise': 'error',
      'jest/prefer-to-be': 'error',
      'jest/prefer-to-have-length': 'error',
      'jest/valid-describe-callback': 'error',
      'jest/valid-title': 'error',

      // Node plugin
      'n/no-missing-import': 'off', // TypeScript handles this
      'n/no-unsupported-features/node-builtins': 'off',
    },
  },
  // Test file configuration
  {
    files: ['**/*.test.ts', '**/test/**/*.ts'],
    languageOptions: {
      globals: {
        describe: 'readonly',
        test: 'readonly',
        it: 'readonly',
        expect: 'readonly',
        beforeEach: 'readonly',
        afterEach: 'readonly',
        beforeAll: 'readonly',
        afterAll: 'readonly',
        fail: 'readonly',
      },
    },
    rules: {
      'jest/no-standalone-expect': 'off',
      'jest/expect-expect': 'off',
    },
  }
)
