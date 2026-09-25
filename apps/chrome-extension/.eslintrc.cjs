module.exports = {
  root: true,
  env: { browser: true, es2021: true, webextensions: true },
  extends: ['eslint:recommended', 'plugin:@typescript-eslint/recommended'],
  parser: '@typescript-eslint/parser',
  parserOptions: { ecmaVersion: 'latest', sourceType: 'module' },
  plugins: ['@typescript-eslint'],
  rules: {
    '@typescript-eslint/no-explicit-any': 'error',
    '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
    'no-restricted-globals': [
      'error',
      { name: 'fetch', message: 'The extension never makes network requests (spec §130).' },
      { name: 'XMLHttpRequest', message: 'The extension never makes network requests.' },
    ],
  },
};
