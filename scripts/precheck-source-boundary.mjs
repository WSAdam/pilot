#!/usr/bin/env node
// pilot: checks, without compiling, the pattern counts that
// moli-renderer-v8/src/webidl_callback_source_boundary_tests.rs freezes.
// That test counts raw V8 entry points (`.run(scope`, `.evaluate(scope`,
// `.call(scope`, `Global<v8::Function>`, microtask scopes) per renderer source
// file, with whitespace removed, and fails when any count changes. Nothing
// compiles on the developer's Mac, so this compares each renderer file the
// branch changes between two revisions and lists every count that moved.
// Build 10's first gate run failed on exactly such a change (util.rs gained a
// `.run(&scope`).
//
// Usage: node scripts/precheck-source-boundary.mjs <base> <head>
// Exit 0 when no count moved, 1 when one did.
import { execFileSync } from 'node:child_process';

const RENDERER_SOURCE = 'moli-renderer-v8/src';
const GUARD_FILE = 'webidl_callback_source_boundary_tests.rs';

// The needles each inventory counts, as the Rust test spells them after
// compact_source() drops whitespace.
export const INVENTORIES = {
  scriptRuns: ['.run(scope', '.run(&scope', '.run(&mutscope'],
  moduleEvaluations: ['.evaluate(scope', '.evaluate(&scope', '.evaluate(&mutscope'],
  microtaskScopes: ['v8::MicrotasksScope::new('],
  sharedMicrotaskBoundaries: ['moli_v8_util::with_microtasks_scope('],
  rawGlobalFunctions: ['Global<v8::Function>', 'Global<Function>'],
  directV8Calls: ['.call(scope', '.call(&scope', '.call(&mutscope'],
};

const git = (cwd, ...args) =>
  execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] });

const compact = (source) => source.replace(/\s+/gu, '');
const countNeedles = (source, needles) => needles.reduce((total, needle) => total + source.split(needle).length - 1, 0);

function fileAt(cwd, revision, file) {
  try {
    return compact(git(cwd, 'show', `${revision}:${file}`));
  } catch {
    return ''; // added or deleted on one side
  }
}

/** Every inventory count that differs between base and head, per changed renderer file. */
export function boundaryCountChanges(base, head, cwd = process.cwd()) {
  const files = git(cwd, 'diff', '--name-only', base, head, '--', RENDERER_SOURCE)
    .split('\n')
    .filter((file) => file.endsWith('.rs') && !file.endsWith(`/${GUARD_FILE}`));
  const changes = [];
  for (const file of files) {
    const before = fileAt(cwd, base, file);
    const after = fileAt(cwd, head, file);
    for (const [inventory, needles] of Object.entries(INVENTORIES)) {
      const from = countNeedles(before, needles);
      const to = countNeedles(after, needles);
      if (from !== to) changes.push({ inventory, file, from, to });
    }
  }
  return { files: files.length, changes };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [base, head] = process.argv.slice(2);
  if (!base || !head) {
    console.error('❌ usage: node scripts/precheck-source-boundary.mjs <base> <head>');
    process.exit(2);
  }
  console.log(`🔍 renderer source-boundary counts, ${base}..${head}`);
  const { files, changes } = boundaryCountChanges(base, head);
  for (const { inventory, file, from, to } of changes) {
    console.log(`❌ ${inventory} ${file}: ${from} -> ${to}`);
  }
  if (changes.length > 0) {
    console.log(`⚠️ ${changes.length} count(s) moved in ${files} changed renderer files; webidl_callback_source_boundary_tests will fail`);
    process.exit(1);
  }
  console.log(`✅ no count moved in ${files} changed renderer files`);
}
