/** Small helpers that give the plain notes box light formatting. Pure, so they can be tested. */

export interface EditResult {
  text: string;
  /** Caret / selection after the edit. */
  selStart: number;
  selEnd: number;
}

export const NOTE_TEMPLATES: { key: string; label: string; body: string }[] = [
  { key: 'agenda', label: 'Agenda', body: '## Agenda\n- \n' },
  { key: 'actions', label: 'Action items', body: '## Action items\n- [ ] \n' },
  { key: 'decisions', label: 'Decisions', body: '## Decisions\n- \n' },
  { key: 'questions', label: 'Open questions', body: '## Open questions\n- \n' },
  { key: 'oneonone', label: '1:1', body: '## Their topics\n- \n\n## My topics\n- \n\n## Follow-ups\n- [ ] \n' },
  { key: 'customer', label: 'Customer call', body: '## Goals\n- \n\n## What we learned\n- \n\n## Next steps\n- [ ] \n' },
];

/** Text of the slash command being typed just before the caret, e.g. "/act" -> "act". */
export function slashQuery(text: string, caret: number): { query: string; start: number } | null {
  const before = text.slice(0, caret);
  const m = /(?:^|\n)(\/[a-z0-9 ]{0,20})$/i.exec(before);
  if (!m) return null;
  const start = caret - m[1].length;
  return { query: m[1].slice(1).toLowerCase(), start };
}

export function matchTemplates(query: string) {
  const q = query.trim().toLowerCase();
  return NOTE_TEMPLATES.filter(t => !q || t.key.startsWith(q) || t.label.toLowerCase().includes(q));
}

/** Replace the "/query" before the caret with a template body. */
export function insertTemplate(text: string, caret: number, start: number, body: string): EditResult {
  const next = text.slice(0, start) + body + text.slice(caret);
  // Put the caret after the first "- " (or at the end of the inserted text).
  const firstBullet = body.indexOf('- ');
  const pos = start + (firstBullet >= 0 ? firstBullet + 2 : body.length);
  return { text: next, selStart: pos, selEnd: pos };
}

const LIST_RE = /^(\s*)(- \[[ x]\] |[-*] |\d+\. )(.*)$/;

/**
 * Enter inside a list line continues the list; Enter on an empty list line ends it.
 * Returns null when the line is not a list item (let the browser handle Enter).
 */
export function continueList(text: string, caret: number): EditResult | null {
  const lineStart = text.lastIndexOf('\n', caret - 1) + 1;
  const line = text.slice(lineStart, caret);
  const m = LIST_RE.exec(line);
  if (!m) return null;
  const [, indent, marker, rest] = m;
  if (rest.trim() === '') {
    // Empty item: remove the marker and leave a blank line.
    const next = text.slice(0, lineStart) + text.slice(caret);
    return { text: next, selStart: lineStart, selEnd: lineStart };
  }
  let nextMarker = marker;
  const num = /^(\d+)\. $/.exec(marker);
  if (num) nextMarker = `${parseInt(num[1], 10) + 1}. `;
  if (marker.startsWith('- [')) nextMarker = '- [ ] ';
  const insert = `\n${indent}${nextMarker}`;
  const next = text.slice(0, caret) + insert + text.slice(caret);
  const pos = caret + insert.length;
  return { text: next, selStart: pos, selEnd: pos };
}

/** Wrap the selection in a marker (**, *), or remove it when already wrapped. */
export function toggleWrap(text: string, start: number, end: number, marker: string): EditResult {
  const m = marker.length;
  const wrapped = text.slice(start - m, start) === marker && text.slice(end, end + m) === marker;
  if (wrapped) {
    const next = text.slice(0, start - m) + text.slice(start, end) + text.slice(end + m);
    return { text: next, selStart: start - m, selEnd: end - m };
  }
  const next = text.slice(0, start) + marker + text.slice(start, end) + marker + text.slice(end);
  return { text: next, selStart: start + m, selEnd: end + m };
}
