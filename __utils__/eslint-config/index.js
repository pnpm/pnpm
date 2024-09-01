import path from 'node:path'
import { fileURLToPath } from 'node:url'
import js from '@eslint/js'
import { FlatCompat } from '@eslint/eslintrc'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const compat = new FlatCompat({
  baseDirectory: __dirname,
  recommendedConfig: js.configs.recommended,
  allConfig: js.configs.all
})

export default [...compat.extends('standard-with-typescript'), {
  files: ['**/*.ts'],

  linterOptions: {
    reportUnusedDisableDirectives: true,
  },

  languageOptions: {
    ecmaVersion: 5,
    sourceType: 'module',

    parserOptions: {
      project: './tsconfig.lint.json',
    },
  },

  rules: {
    'import/no-extraneous-dependencies': ['error', {
      devDependencies: ['**/pnpm/src/**', '**/test/**', '**/src/**/*.test.ts'],
    }],

    'import/no-default-export': 'error',

    '@typescript-eslint/indent': ['error', 2, {
      FunctionDeclaration: {
        parameters: 'first',
      },

      FunctionExpression: {
        parameters: 'first',
      },
    }],

    '@typescript-eslint/consistent-indexed-object-style': 'off',
    '@typescript-eslint/prefer-reduce-type-parameter': 'off',
    '@typescript-eslint/naming-convention': 'error',
    '@typescript-eslint/explicit-function-return-type': 'off',
    '@typescript-eslint/no-explicit-any': 'error',
    'no-return-await': 'error',
    'no-await-in-loop': 'error',
    '@typescript-eslint/return-await': 'off',
    '@typescript-eslint/no-require-imports': 'error',
    '@typescript-eslint/no-unused-expressions': 'error',
    '@typescript-eslint/no-use-before-define': 'error',
    '@typescript-eslint/no-var-requires': 'error',

    '@typescript-eslint/quotes': ['error', 'single', {
      avoidEscape: true,
    }],

    '@typescript-eslint/triple-slash-reference': 'off',
    '@typescript-eslint/no-non-null-assertion': 'off',
    '@typescript-eslint/consistent-type-assertions': 'off',
    '@typescript-eslint/strict-boolean-expressions': 'off',
    '@typescript-eslint/no-base-to-string': 'off',
    '@typescript-eslint/no-dynamic-delete': 'off',
    '@typescript-eslint/promise-function-async': 'off',
    '@typescript-eslint/no-misused-promises': 'off',
    'no-multi-str': 'off',
    'no-mixed-operators': 'off',
    '@typescript-eslint/dot-notation': 'off',
    '@typescript-eslint/no-unnecessary-type-assertion': 'off',
    '@typescript-eslint/type-annotation-spacing': 'error',
    '@typescript-eslint/ban-ts-comment': 'off',
    '@typescript-eslint/explicit-module-boundary-types': 'error',
    'brace-style': ['error', '1tbs'],

    '@typescript-eslint/comma-dangle': ['error', {
      arrays: 'always-multiline',
      exports: 'always-multiline',
      functions: 'never',
      imports: 'always-multiline',
      objects: 'always-multiline',
    }],

    curly: 'off',
    'eol-last': 'off',
    'import/order': 'off',
    'max-len': 'off',
    'no-multiple-empty-lines': 'error',
    'no-redeclare': 'error',

    'no-restricted-properties': ['error', {
      property: 'substr',
      message: 'Use .slice instead of .substr.',
    }],

    'no-trailing-spaces': 'error',
    'no-var': 'error',
    'no-lone-blocks': 'off',
    'space-before-function-paren': ['error', 'always'],
  },
}]
