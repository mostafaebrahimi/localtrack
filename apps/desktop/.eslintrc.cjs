module.exports = {
  root: true,
  env: { browser: true, es2021: true },
  extends: [
    'eslint:recommended',
    'plugin:@typescript-eslint/recommended',
    'plugin:react-hooks/recommended',
  ],
  parser: '@typescript-eslint/parser',
  parserOptions: { ecmaVersion: 'latest', sourceType: 'module' },
  plugins: ['@typescript-eslint', 'react-hooks'],
  rules: {
    '@typescript-eslint/no-explicit-any': 'error',
    '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
    'no-restricted-globals': [
      'error',
      { name: 'fetch', message: 'LocalTrack makes no network calls (spec §129/§130).' },
    ],
    'no-restricted-properties': [
      'error',
      { object: 'window', property: 'fetch', message: 'LocalTrack makes no network calls.' },
    ],
  },
};
