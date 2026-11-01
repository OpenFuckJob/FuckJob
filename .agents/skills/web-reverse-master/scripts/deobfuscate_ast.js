#!/usr/bin/env node
/**
 * AST 反混淆最小管线 — 自包含 Babel 反混淆脚本。
 *
 * 安装依赖（一次性）:
 *   npm install @babel/parser @babel/generator @babel/traverse @babel/types
 *
 * 用法:
 *   node deobfuscate_ast.js input.js [output.js]
 *
 * Pass 1: 内联字符串常量（_0x1234('0x1a2b') → 'actual_string'）
 * Pass 2: 删除死代码分支（if(true)/if(false)/if(!![])）
 * Pass 3: 简化表达式（字符串拼接/数字运算/!![]→true）
 */

const parser = require('@babel/parser');
const generate = require('@babel/generator').default;
const traverse = require('@babel/traverse').default;
const t = require('@babel/types');
const fs = require('fs');

const inputFile = process.argv[2];
const outputFile = process.argv[3] || inputFile.replace(/\.js$/, '_deobf.js');

const code = fs.readFileSync(inputFile, 'utf-8');

// Step 1: 解析
const ast = parser.parse(code, {
    sourceType: 'unambiguous',
    errorRecovery: true,
});

let changes = 0;

// === Pass 1: 内联字符串常量 ===
traverse(ast, {
    CallExpression(path) {
        if (
            t.isIdentifier(path.node.callee) &&
            path.node.callee.name.startsWith('_0x') &&
            path.node.arguments.length === 1 &&
            t.isStringLiteral(path.node.arguments[0]) &&
            path.node.arguments[0].value.startsWith('0x')
        ) {
            try {
                const evalCode = generate(path.node).code;
                const result = eval(evalCode);
                if (typeof result === 'string') {
                    path.replaceWith(t.stringLiteral(result));
                    changes++;
                }
            } catch (e) {
                // 解码失败，保持原样
            }
        }
    },
});

// === Pass 2: 删除死代码分支 ===
traverse(ast, {
    IfStatement(path) {
        const test = path.node.test;

        // if (true) { ... } → 保留 consequent
        if (t.isBooleanLiteral(test) && test.value === true) {
            if (path.node.consequent) {
                path.replaceWithMultiple(
                    t.isBlockStatement(path.node.consequent)
                        ? path.node.consequent.body
                        : [path.node.consequent]
                );
                changes++;
            }
        }

        // if (false) { ... } → 移除（或用 alternate）
        if (t.isBooleanLiteral(test) && test.value === false) {
            if (path.node.alternate) {
                path.replaceWithMultiple(
                    t.isBlockStatement(path.node.alternate)
                        ? path.node.alternate.body
                        : [path.node.alternate]
                );
            } else {
                path.remove();
            }
            changes++;
        }

        // if (!![]) → true（JSFuck风格）
        if (
            t.isUnaryExpression(test) &&
            test.operator === '!' &&
            t.isUnaryExpression(test.argument) &&
            test.argument.operator === '!' &&
            t.isArrayExpression(test.argument.argument)
        ) {
            if (path.node.consequent) {
                path.replaceWithMultiple(
                    t.isBlockStatement(path.node.consequent)
                        ? path.node.consequent.body
                        : [path.node.consequent]
                );
                changes++;
            }
        }
    },
});

// === Pass 3: 简化表达式 ===
traverse(ast, {
    BinaryExpression(path) {
        const { left, right, operator } = path.node;

        // 字符串拼接: 'a' + 'b' → 'ab'
        if (operator === '+' && t.isStringLiteral(left) && t.isStringLiteral(right)) {
            path.replaceWith(t.stringLiteral(left.value + right.value));
            changes++;
        }

        // 数字运算: 0x1a + 0x2b → 数字
        if (
            ['+', '-', '*', '/', '%'].includes(operator) &&
            t.isNumericLiteral(left) &&
            t.isNumericLiteral(right)
        ) {
            const result = eval(`${left.value} ${operator} ${right.value}`);
            path.replaceWith(t.numericLiteral(result));
            changes++;
        }
    },

    // !![] → true, ![] → false
    UnaryExpression(path) {
        if (
            path.node.operator === '!' &&
            t.isUnaryExpression(path.node.argument) &&
            path.node.argument.operator === '!' &&
            t.isArrayExpression(path.node.argument.argument)
        ) {
            path.replaceWith(t.booleanLiteral(true));
            changes++;
        }
        if (path.node.operator === '!' && t.isArrayExpression(path.node.argument)) {
            path.replaceWith(t.booleanLiteral(false));
            changes++;
        }
    },
});

// Step 4: 生成代码
const output = generate(ast, {
    compact: false,
    comments: true,
    retainLines: false,
}).code;

fs.writeFileSync(outputFile, output);
console.log(`[DONE] ${changes} 处修改 → ${outputFile}`);
