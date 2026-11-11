// Hooks after an early return (React #310「Rendered more hooks than during the previous render」: the page shows
// 「这一页出错了」 on the render where the data arrives). Walks every component / hook in src/ and lists the hook
// calls that come after a top-level `if (…) return` of the same function. No eslint in this project; run after
// touching components:  node scripts/hooks-lint.mjs   (exit 1 when something is found)
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import ts from "typescript";

const files = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const p = path.join(dir, name);
    if (statSync(p).isDirectory()) name === "vendor" || walk(p);
    else if (/\.tsx?$/.test(name)) files.push(p);
  }
};
walk("src");

const isHook = (name) => /^use[A-Z0-9]/.test(name);
const isOurs = (name) => /^[A-Z]/.test(name) || isHook(name);
/** A hook call anywhere in `node`, not inside a nested function. */
function hookIn(node) {
  let found = null;
  const visit = (n) => {
    if (found || ts.isFunctionLike(n)) return;
    if (ts.isCallExpression(n)) {
      const c = n.expression;
      const name = ts.isIdentifier(c) ? c.text : ts.isPropertyAccessExpression(c) ? c.name.text : "";
      if (isHook(name)) return void (found = n);
    }
    ts.forEachChild(n, visit);
  };
  visit(node);
  return found;
}
/** A `return` in `node` that leaves the function (not inside a nested function). */
function returns(node) {
  let found = false;
  const visit = (n) => {
    if (found || ts.isFunctionLike(n)) return;
    if (ts.isReturnStatement(n)) return void (found = true);
    ts.forEachChild(n, visit);
  };
  visit(node);
  return found;
}

let bad = 0;
for (const file of files) {
  const src = ts.createSourceFile(file, readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, file.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const check = (name, body) => {
    if (!body || !ts.isBlock(body)) return;
    let left = null;
    for (const st of body.statements) {
      // a hook in a statement after one that may have returned
      if (left) {
        const h = hookIn(st);
        if (h) {
          const at = src.getLineAndCharacterOfPosition(h.getStart());
          console.log(`${file}:${at.line + 1}  ${name}: ${h.expression.getText()} after the return at line ${left}`);
          bad += 1;
          return;
        }
      }
      if (!left && !ts.isReturnStatement(st) && returns(st)) left = src.getLineAndCharacterOfPosition(st.getStart()).line + 1;
    }
  };
  const visit = (n) => {
    if (ts.isFunctionDeclaration(n) && n.name && isOurs(n.name.text)) check(n.name.text, n.body);
    if (ts.isVariableDeclaration(n) && ts.isIdentifier(n.name) && isOurs(n.name.text) && n.initializer) {
      let f = n.initializer;
      // forwardRef(function …) / memo(…)
      if (ts.isCallExpression(f) && f.arguments[0]) f = f.arguments[0];
      if (ts.isArrowFunction(f) || ts.isFunctionExpression(f)) check(n.name.text, f.body);
    }
    ts.forEachChild(n, visit);
  };
  visit(src);
}
console.log(bad ? `${bad} 处` : "没有写在提前返回后面的 hook");
process.exit(bad ? 1 : 0);
