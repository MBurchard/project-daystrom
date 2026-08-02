export default {
  extends: ['stylelint-config-standard'],
  plugins: ['@stylistic/stylelint-plugin'],
  defaultSeverity: 'warning',
  overrides: [
    {
      files: ['**/*.vue'],
      customSyntax: 'postcss-html',
      rules: {
        'selector-pseudo-class-no-unknown': [true, {ignorePseudoClasses: ['deep', 'global']}],
      },
    },
  ],
  rules: {
    '@stylistic/selector-list-comma-newline-after': 'always-multi-line',
    '@stylistic/string-quotes': 'double',
    'custom-property-pattern': '^([a-z][a-z0-9]*)(-[a-z0-9*]+)*$',
  },
};
