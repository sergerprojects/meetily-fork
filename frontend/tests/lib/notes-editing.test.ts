import { describe, expect, test } from 'bun:test';
import { continueList, insertTemplate, matchTemplates, slashQuery, toggleWrap } from '../../src/lib/notes-editing';

describe('slash templates', () => {
  test('finds the command being typed at the start of a line', () => {
    expect(slashQuery('hello\n/act', 10)).toEqual({ query: 'act', start: 6 });
    expect(slashQuery('/', 1)).toEqual({ query: '', start: 0 });
  });
  test('ignores slashes inside a sentence', () => {
    expect(slashQuery('and/or', 6)).toBeNull();
    expect(slashQuery('see https://x.com/path', 22)).toBeNull();
  });
  test('matches by key prefix or label', () => {
    expect(matchTemplates('act').map(t => t.key)).toEqual(['actions']);
    expect(matchTemplates('').length).toBeGreaterThan(3);
    expect(matchTemplates('zzz')).toEqual([]);
  });
  test('inserting replaces the command and parks the caret in the first bullet', () => {
    const r = insertTemplate('notes\n/agenda', 13, 6, '## Agenda\n- \n');
    expect(r.text).toBe('notes\n## Agenda\n- \n');
    expect(r.text.slice(r.selStart - 2, r.selStart)).toBe('- ');
  });
});

describe('lists', () => {
  test('enter continues a bullet', () => {
    const r = continueList('- one', 5)!;
    expect(r.text).toBe('- one\n- ');
    expect(r.selStart).toBe(8);
  });
  test('enter continues numbers and resets checkboxes', () => {
    expect(continueList('2. two', 6)!.text).toBe('2. two\n3. ');
    expect(continueList('- [x] done', 10)!.text).toBe('- [x] done\n- [ ] ');
  });
  test('enter on an empty item ends the list', () => {
    const r = continueList('- one\n- ', 8)!;
    expect(r.text).toBe('- one\n');
  });
  test('enter outside a list is left alone', () => {
    expect(continueList('plain text', 10)).toBeNull();
  });
  test('indentation is kept', () => {
    expect(continueList('  - nested', 10)!.text).toBe('  - nested\n  - ');
  });
});

describe('bold and italics', () => {
  test('wraps and unwraps a selection', () => {
    const on = toggleWrap('a word b', 2, 6, '**');
    expect(on.text).toBe('a **word** b');
    const off = toggleWrap(on.text, on.selStart, on.selEnd, '**');
    expect(off.text).toBe('a word b');
  });
});
