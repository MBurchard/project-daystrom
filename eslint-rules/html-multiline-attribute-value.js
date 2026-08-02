function leadingSpaces(line) {
  const match = line.match(/^ */u);
  return match ? match[0].length : 0;
}

function linePrefix(sourceCode, index, line) {
  return sourceCode.text.slice(index - line.column, index);
}

function lineSuffix(sourceCode, index, line) {
  const lineText = sourceCode.lines[line.line - 1] ?? '';
  return lineText.slice(line.column);
}

function isAllowedSuffix(suffix) {
  const trimmed = suffix.trim();
  return trimmed === '' || trimmed === '>' || trimmed === '/>';
}

function spaces(count) {
  return ' '.repeat(count);
}

function previousInlineWhitespaceStart(text, index) {
  let start = index;
  while (start > 0 && /[ \t]/u.test(text[start - 1])) {
    start -= 1;
  }

  return start;
}

function normalizeAttributeValue(value) {
  return value.trim().split(/\s+/u).filter(Boolean);
}

function normalizeCspDirectives(value) {
  return value
    .split(';')
    .map(directive => directive.trim().replace(/\s+/gu, ' '))
    .filter(Boolean)
    .map(directive => `${directive};`);
}

function splitStyleDeclarations(value) {
  const declarations = [];
  let start = 0;
  let quote = '';
  let escaped = false;
  let depth = 0;

  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];

    if (escaped) {
      escaped = false;
      continue;
    }

    if (character === '\\') {
      escaped = true;
      continue;
    }

    if (quote) {
      if (character === quote) {
        quote = '';
      }
      continue;
    }

    if (character === '"' || character === '\'') {
      quote = character;
      continue;
    }

    if (character === '(' || character === '[' || character === '{') {
      depth += 1;
      continue;
    }

    if (character === ')' || character === ']' || character === '}') {
      depth = Math.max(0, depth - 1);
      continue;
    }

    if (character === ';' && depth === 0) {
      declarations.push({
        content: value.slice(start, index),
        terminated: true,
      });
      start = index + 1;
    }
  }

  declarations.push({
    content: value.slice(start),
    terminated: false,
  });

  return declarations;
}

function normalizeStyleWhitespace(value) {
  let normalized = '';
  let quote = '';
  let escaped = false;
  let pendingSpace = false;

  for (const character of value.trim()) {
    if (escaped) {
      normalized += character;
      escaped = false;
      continue;
    }

    if (character === '\\') {
      normalized += character;
      escaped = true;
      continue;
    }

    if (quote) {
      normalized += character;
      if (character === quote) {
        quote = '';
      }
      continue;
    }

    if (character === '"' || character === '\'') {
      if (pendingSpace && normalized) {
        normalized += ' ';
      }
      pendingSpace = false;
      normalized += character;
      quote = character;
      continue;
    }

    if (/\s/u.test(character)) {
      pendingSpace = true;
      continue;
    }

    if (pendingSpace && normalized) {
      normalized += ' ';
    }
    pendingSpace = false;
    normalized += character;
  }

  return normalized;
}

function normalizeStyleDeclaration(declaration) {
  const separator = declaration.indexOf(':');
  if (separator < 0) {
    return normalizeStyleWhitespace(declaration);
  }

  const property = declaration.slice(0, separator).trim();
  const value = normalizeStyleWhitespace(declaration.slice(separator + 1));
  return `${property}: ${value}`;
}

function normalizeStyleDeclarations(value) {
  return splitStyleDeclarations(value)
    .map(({content, terminated}) => {
      const declaration = normalizeStyleDeclaration(content);
      return declaration ? `${declaration}${terminated ? ';' : ''}` : '';
    })
    .filter(Boolean);
}

function attributeName(node) {
  return typeof node.key.name === 'string' ? node.key.name.toLowerCase() : '';
}

function attributeValue(node) {
  return typeof node.value?.value === 'string' ? node.value.value : '';
}

function isContentSecurityPolicyContent(node) {
  if (attributeName(node) !== 'content') {
    return false;
  }

  const startTag = node.parent;
  const element = startTag?.parent;
  if (element?.rawName?.toLowerCase() !== 'meta') {
    return false;
  }

  return startTag.attributes.some(attribute =>
    attributeName(attribute) === 'http-equiv' &&
    attributeValue(attribute).toLowerCase() === 'content-security-policy',
  );
}

function renderAttributeLine(name, tokens, isFirstLine, continuationIndent) {
  const content = tokens.join(' ');
  if (isFirstLine) {
    return `${name}="${content}`;
  }

  return `${spaces(continuationIndent)}${content}`;
}

function formatAttribute(name, tokens, baseIndent, continuationIndent, maxLineLength) {
  if (tokens.length === 0) {
    return `${name}=""`;
  }

  const lines = [];
  let currentTokens = [];

  for (const token of tokens) {
    const proposedTokens = [...currentTokens, token];
    const isFirstLine = lines.length === 0;
    const proposedLine = renderAttributeLine(name, proposedTokens, isFirstLine, continuationIndent);
    const proposedLength = (isFirstLine ? baseIndent : 0) + proposedLine.length + 1;

    if (currentTokens.length > 0 && proposedLength > maxLineLength) {
      lines.push(renderAttributeLine(name, currentTokens, isFirstLine, continuationIndent));
      currentTokens = [token];
    } else {
      currentTokens = proposedTokens;
    }
  }

  lines.push(renderAttributeLine(name, currentTokens, lines.length === 0, continuationIndent));
  lines[lines.length - 1] = `${lines[lines.length - 1]}"`;

  return lines.join('\n');
}

// noinspection JSUnusedGlobalSymbols
export default {
  meta: {
    type: 'layout',
    fixable: 'whitespace',
    docs: {
      description: 'Enforce standalone, consistently indented multiline HTML attribute values.',
    },
    schema: [{
      type: 'object',
      properties: {
        indent: {
          type: 'integer',
          minimum: 0,
        },
        maxLineLength: {
          type: 'integer',
          minimum: 1,
        },
      },
      additionalProperties: false,
    }],
    messages: {
      attributeFormat: 'Static attribute values must be consistently wrapped.',
      ownLine: 'Multiline attribute values must start on their own line.',
      noTrailingAttribute: 'Multiline attribute values must not share their closing line with another attribute.',
      valueIndent: 'Multiline attribute value continuation should be indented {{expected}} spaces.',
    },
  },
  create(context) {
    const sourceCode = context.sourceCode;
    const [{indent = 2, maxLineLength = 120} = {}] = context.options;

    function checkStaticAttribute(node) {
      const name = node.key.name;
      const prefix = linePrefix(sourceCode, node.range[0], node.loc.start);
      const suffix = lineSuffix(sourceCode, node.range[1], node.loc.end);
      const startsOwnLine = prefix.trim() === '';
      const cspContent = isContentSecurityPolicyContent(node);
      const styleAttribute = attributeName(node) === 'style';
      let tokens = normalizeAttributeValue(node.value.value);
      if (cspContent) {
        tokens = normalizeCspDirectives(node.value.value);
      } else if (styleAttribute) {
        tokens = normalizeStyleDeclarations(node.value.value);
      }

      const compactContent = tokens.join(' ');
      const compactAttribute = `${name}="${compactContent}"`;
      const isMultiline = sourceCode.getText(node).includes('\n');
      const needsWrapping = isMultiline || node.loc.start.column + compactAttribute.length > maxLineLength;
      const needsStyleNormalization = styleAttribute && sourceCode.getText(node) !== compactAttribute;

      if (!needsWrapping && !needsStyleNormalization) {
        return;
      }

      const baseIndent = startsOwnLine ?
        node.loc.start.column :
          (node.parent?.loc?.start.column ?? node.loc.start.column) + indent;
      const continuationIndent = baseIndent + indent;
      const formattedAttribute = formatAttribute(name, tokens, baseIndent, continuationIndent, maxLineLength);

      const currentAttribute = sourceCode.getText(node);

      if (!needsWrapping) {
        context.report({
          node,
          messageId: 'attributeFormat',
          fix(fixer) {
            return fixer.replaceText(node, formattedAttribute);
          },
        });
        return;
      }

      if (currentAttribute === formattedAttribute && startsOwnLine && isAllowedSuffix(suffix)) {
        return;
      }

      context.report({
        node,
        messageId: 'attributeFormat',
        fix(fixer) {
          const start = startsOwnLine ?
            node.range[0] :
              previousInlineWhitespaceStart(sourceCode.text, node.range[0]);
          const prefixReplacement = startsOwnLine ? '' : `\n${spaces(baseIndent)}`;
          const suffixWhitespace = suffix.match(/^[ \t]*/u)?.[0] ?? '';
          const hasTrailingAttribute = !isAllowedSuffix(suffix);
          const end = hasTrailingAttribute ?
            node.range[1] + suffixWhitespace.length :
            node.range[1];
          const suffixReplacement = hasTrailingAttribute ? `\n${spaces(baseIndent)}` : '';

          return fixer.replaceTextRange(
            [start, end],
            `${prefixReplacement}${formattedAttribute}${suffixReplacement}`,
          );
        },
      });
    }

    function checkAttribute(node) {
      if (node.directive) {
        return;
      }

      if (!node.value) {
        return;
      }

      if (typeof node.key.name === 'string' && typeof node.value.value === 'string') {
        checkStaticAttribute(node);
        return;
      }

      const attributeText = sourceCode.getText(node);
      if (!attributeText.includes('\n')) {
        return;
      }

      const prefix = linePrefix(sourceCode, node.range[0], node.loc.start);
      if (prefix.trim() !== '') {
        context.report({
          node,
          messageId: 'ownLine',
          fix(fixer) {
            const attributeIndent = (node.parent?.loc?.start.column ?? node.loc.start.column) + indent;
            const whitespaceStart = previousInlineWhitespaceStart(sourceCode.text, node.range[0]);

            return fixer.replaceTextRange([whitespaceStart, node.range[0]], `\n${spaces(attributeIndent)}`);
          },
        });
      }

      const suffix = lineSuffix(sourceCode, node.range[1], node.loc.end);
      if (!isAllowedSuffix(suffix)) {
        context.report({
          node,
          loc: node.loc.end,
          messageId: 'noTrailingAttribute',
          fix(fixer) {
            const whitespaceEnd = node.range[1] + (suffix.match(/^[ \t]*/u)?.[0].length ?? 0);

            return fixer.replaceTextRange([node.range[1], whitespaceEnd], `\n${spaces(node.loc.start.column)}`);
          },
        });
      }

      const expectedIndent = node.loc.start.column + indent;
      const attributeLines = attributeText.split('\n');
      for (let index = 1; index < attributeLines.length; index += 1) {
        const line = attributeLines[index];
        if (line.trim() === '') {
          continue;
        }

        const actualIndent = leadingSpaces(line);
        if (actualIndent !== expectedIndent) {
          // noinspection JSUnusedGlobalSymbols
          context.report({
            node,
            loc: {
              line: node.loc.start.line + index,
              column: actualIndent,
            },
            messageId: 'valueIndent',
            data: {
              expected: `${expectedIndent}`,
            },
            fix(fixer) {
              const lineStart = sourceCode.getIndexFromLoc({
                line: node.loc.start.line + index,
                column: 0,
              });

              return fixer.replaceTextRange([lineStart, lineStart + actualIndent], spaces(expectedIndent));
            },
          });
        }
      }
    }

    const templateVisitor = {
      VAttribute: checkAttribute,
    };

    const parserServices = sourceCode.parserServices;
    if (sourceCode.ast.templateBody && parserServices?.defineTemplateBodyVisitor) {
      return parserServices.defineTemplateBodyVisitor(templateVisitor);
    }

    if (parserServices?.defineDocumentVisitor) {
      return parserServices.defineDocumentVisitor(templateVisitor);
    }

    return templateVisitor;
  },
};
