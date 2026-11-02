/**
 * Debug: manually run normalize with sourceType tracking
 */
const fs = require('fs');
const parser = require('@babel/parser');
const traverse = require('@babel/traverse').default;
const generator = require('@babel/generator').default;
const t = require('@babel/types');

const inputPath = 'e:/PythonCodeObject1/Qoder_ObjectProdemo2/sites/zhipin/assets/js/70c092c1.js';
const outputPath = 'e:/PythonCodeObject1/Qoder_ObjectProdemo2/sites/zhipin/assets/js-deob/01_normalize_v2.js';

function toStatements(expressions) { return expressions.map(expr => t.expressionStatement(expr)); }
function hasUnsafeContinue(bodyPath) { let found = false; bodyPath.traverse({ Function(p) { p.skip(); }, ContinueStatement(p) { found = true; p.stop(); } }); return found; }
function ensureBlock(path) { if (path.isBlockStatement()) return path; path.replaceWith(t.blockStatement([path.node])); return path.parentPath.get(path.key); }
function replaceStatement(path, node) { if (path.inList) { path.replaceWith(node); return; } path.replaceWith(t.blockStatement([node])); }
function replaceStatementWithStatements(path, statements) {
  if (statements.length === 0) { path.remove(); return; }
  if (path.inList) { path.replaceWithMultiple(statements); return; }
  if (statements.length === 1) { path.replaceWith(statements[0]); return; }
  path.replaceWith(t.blockStatement(statements));
}

function normalize(ast) {
  let changed = false;
  traverse(ast, {
    UnaryExpression(path) {
      if (path.node.operator === 'void' && path.parentPath.isExpressionStatement()) {
        path.replaceWith(path.node.argument); changed = true;
      }
    },
    SequenceExpression(path) {
      if (path.parentPath.isExpressionStatement()) {
        replaceStatementWithStatements(path.parentPath, toStatements(path.node.expressions)); changed = true; return;
      }
      if (path.parentPath.isReturnStatement()) {
        if (!path.parentPath.inList) return;
        const exprs = path.node.expressions.slice(); const returnExpr = exprs.pop();
        path.parentPath.insertBefore(toStatements(exprs)); path.replaceWith(returnExpr); changed = true; return;
      }
      if (path.parentPath.isIfStatement() && path.key === 'test') {
        if (!path.parentPath.inList) return;
        const exprs = path.node.expressions.slice(); const testExpr = exprs.pop();
        path.parentPath.insertBefore(toStatements(exprs)); path.replaceWith(testExpr); changed = true; return;
      }
      if (path.parentPath.isForStatement() && path.key === 'init') {
        if (!path.parentPath.inList) return;
        path.parentPath.insertBefore(toStatements(path.node.expressions)); path.remove(); changed = true; return;
      }
      if (path.parentPath.isForStatement() && path.key === 'test') {
        const exprs = path.node.expressions.slice(); const testExpr = exprs.pop();
        const forPath = path.parentPath; const bodyPath = ensureBlock(forPath.get('body'));
        bodyPath.unshiftContainer('body', t.ifStatement(t.unaryExpression('!', testExpr, true), t.blockStatement([t.breakStatement()])));
        bodyPath.unshiftContainer('body', toStatements(exprs)); path.remove(); changed = true; return;
      }
      if (path.parentPath.isForStatement() && path.key === 'update') {
        const forPath = path.parentPath; const bodyPath = ensureBlock(forPath.get('body'));
        if (hasUnsafeContinue(bodyPath)) return;
        bodyPath.pushContainer('body', toStatements(path.node.expressions)); path.remove(); changed = true;
      }
    },
    ConditionalExpression(path) {
      if (path.parentPath.isExpressionStatement()) {
        replaceStatement(path.parentPath, t.ifStatement(path.node.test, t.blockStatement([t.expressionStatement(path.node.consequent)]), t.blockStatement([t.expressionStatement(path.node.alternate)])));
        changed = true;
      }
    },
    LogicalExpression(path) {
      if (!path.parentPath.isExpressionStatement()) return;
      const body = t.blockStatement([t.expressionStatement(path.node.right)]);
      if (path.node.operator === '&&') { replaceStatement(path.parentPath, t.ifStatement(path.node.left, body, null)); }
      else if (path.node.operator === '||') { replaceStatement(path.parentPath, t.ifStatement(t.unaryExpression('!', path.node.left, true), body, null)); }
      else return;
      changed = true;
    },
    AssignmentExpression(path) {
      if (!path.parentPath.isExpressionStatement() || !t.isConditionalExpression(path.node.right)) return;
      const test = path.node.right.test;
      const consequent = t.expressionStatement(t.assignmentExpression(path.node.operator, t.cloneNode(path.node.left, true), path.node.right.consequent));
      const alternate = t.expressionStatement(t.assignmentExpression(path.node.operator, t.cloneNode(path.node.left, true), path.node.right.alternate));
      replaceStatement(path.parentPath, t.ifStatement(test, t.blockStatement([consequent]), t.blockStatement([alternate])));
      changed = true;
    },
    BlockStatement(path) {
      if (path.parentPath.isSwitchCase()) { path.replaceWithMultiple(path.node.body); changed = true; }
    }
  });
  return { ast, changed };
}

// Step 1: parse the original file
const srcCode = fs.readFileSync(inputPath, 'utf8');
let ast = parser.parse(srcCode, { sourceType: 'script', plugins: ['jsx'] });
console.log('Initial parse OK. Body type:', ast.program.type);

let changed = false;
let iteration = 0;
do {
  iteration++;
  console.log(`\n--- Iteration ${iteration} ---`);
  ({ ast, changed } = normalize(ast));
  console.log(`Changed: ${changed}`);
  
  if (changed) {
    // Generate code
    const genCode = generator(ast, { compact: false, comments: false, jsescOption: { minimal: true } }).code;
    console.log(`Generated: ${genCode.length} chars`);
    
    // Save for debugging
    if (iteration === 1) {
      fs.writeFileSync(outputPath + '.gen1.js', genCode);
    }
    
    // Reparse
    try {
      ast = parser.parse(genCode, { sourceType: 'script', plugins: ['jsx'] });
      console.log(`Reparse OK`);
    } catch (e) {
      console.log(`REPARSE ERROR at iter ${iteration}: ${e.message.substring(0, 200)}`);
      console.log(`Error pos: ${e.pos} / ${genCode.length}`);
      // Show context around error
      const ctx = genCode.substring(Math.max(0, e.pos - 80), Math.min(genCode.length, e.pos + 80));
      console.log(`Context: ${JSON.stringify(ctx)}`);
      
      // Save the failing generated code
      fs.writeFileSync(outputPath + '.failing.js', genCode);
      process.exit(1);
    }
  }
  
  if (iteration > 10) {
    console.log('Too many iterations, stopping');
    break;
  }
} while (changed);

// Save final output
const finalCode = generator(ast, { compact: false, comments: false, jsescOption: { minimal: true } }).code;
fs.writeFileSync(outputPath, finalCode);
console.log(`\nDone. ${iteration} iterations. Output: ${outputPath} (${finalCode.length} chars)`);
