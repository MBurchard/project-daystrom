import {Linter} from 'eslint';
import {describe, expect, it} from 'vitest';
import vueParser from 'vue-eslint-parser';
import rule from './html-multiline-attribute-value.js';

function format(source, maxLineLength = 120) {
  const linter = new Linter();
  return linter.verifyAndFix(source, {
    files: ['**/*.html'],
    languageOptions: {
      parser: vueParser,
    },
    plugins: {
      html: {
        rules: {
          'multiline-attribute-value': rule,
        },
      },
    },
    rules: {
      'html/multiline-attribute-value': ['warn', {indent: 2, maxLineLength}],
    },
  }, {
    filename: 'template.html',
  });
}

describe('html-multiline-attribute-value', () => {
  it('normalizes compact inline styles', () => {
    const result = format('<p style="margin:0;font-size:15px;">Text</p>');

    expect(result.fixed).toBe(true);
    expect(result.output).toBe('<p style="margin: 0; font-size: 15px;">Text</p>');
  });

  it('wraps styles between declarations', () => {
    const source = [
      '<body',
      '  style="margin: 0; padding: 24px; background-color: #ffffff; font-family: Arial, Helvetica, sans-serif;',
      '    font-size: 15px; line-height: 1.5; color: #333333;">',
    ].join('\n');
    const result = format(source);

    expect(result.fixed).toBe(false);
    expect(result.messages).toHaveLength(0);
  });

  it('preserves semicolons inside CSS values', () => {
    const source = '<p style="content:\'a;b\';background-image:url(\'data:image/svg+xml;utf8,test\');">Text</p>';
    const result = format(source);

    expect(result.output).toContain('content: \'a;b\';');
    expect(result.output).toContain('background-image: url(\'data:image/svg+xml;utf8,test\');');
  });
});
