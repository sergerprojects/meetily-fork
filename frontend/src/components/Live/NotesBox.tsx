'use client';

import { KeyboardEvent, useRef, useState } from 'react';
import {
  continueList,
  insertTemplate,
  matchTemplates,
  slashQuery,
  toggleWrap,
  EditResult,
} from '@/lib/notes-editing';

interface Props {
  value: string;
  onChange: (text: string) => void;
}

/**
 * Plain notes box with light help: Enter continues a list, ⌘B / ⌘I wrap the
 * selection, and "/" at the start of a line offers templates. Stored as plain text
 * (markdown-ish), so the importer reads it unchanged.
 */
export function NotesBox({ value, onChange }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const [caret, setCaret] = useState(0);
  const [pick, setPick] = useState(0);
  const [dismissed, setDismissed] = useState<number | null>(null);

  const slash = slashQuery(value, caret);
  const options = slash && dismissed !== slash.start ? matchTemplates(slash.query) : [];

  const apply = (r: EditResult) => {
    onChange(r.text);
    requestAnimationFrame(() => {
      const el = ref.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(r.selStart, r.selEnd);
      setCaret(r.selEnd);
    });
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    if (options.length > 0 && slash) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setPick(p => (p + 1) % options.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setPick(p => (p - 1 + options.length) % options.length);
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        apply(insertTemplate(value, el.selectionStart, slash.start, options[pick % options.length].body));
        setPick(0);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setDismissed(slash.start);
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.metaKey && el.selectionStart === el.selectionEnd) {
      const r = continueList(value, el.selectionStart);
      if (r) {
        e.preventDefault();
        apply(r);
      }
      return;
    }
    if (e.metaKey && !e.altKey && (e.key === 'b' || e.key === 'i')) {
      e.preventDefault();
      apply(toggleWrap(value, el.selectionStart, el.selectionEnd, e.key === 'b' ? '**' : '*'));
    }
  };

  return (
    <div className="relative">
      {options.length > 0 && (
        <ul
          className="absolute left-0 z-20 w-64 overflow-hidden rounded-lg border border-gray-200 bg-white py-1 shadow-lg"
          style={{ top: Math.min(value.slice(0, caret).split('\n').length, 30) * 28 + 4 }}
          role="listbox"
          aria-label="Templates"
        >
          {options.map((t, i) => (
            <li
              key={t.key}
              role="option"
              aria-selected={i === pick % options.length}
              onMouseDown={e => {
                e.preventDefault();
                if (slash) apply(insertTemplate(value, caret, slash.start, t.body));
              }}
              className={`cursor-pointer px-3 py-1.5 text-sm ${i === pick % options.length ? 'bg-gray-100 text-gray-900' : 'text-gray-700'}`}
            >
              /{t.key} <span className="text-gray-400">{t.label}</span>
            </li>
          ))}
        </ul>
      )}
      <textarea
        ref={ref}
        value={value}
        onChange={e => {
          onChange(e.target.value);
          setCaret(e.target.selectionStart);
          setPick(0);
          setDismissed(null);
        }}
        onKeyDown={onKeyDown}
        onKeyUp={e => setCaret(e.currentTarget.selectionStart)}
        onClick={e => setCaret(e.currentTarget.selectionStart)}
        placeholder="Write notes. Type / for templates. The transcript is being captured in the background."
        className="min-h-[55vh] w-full resize-none bg-transparent text-[15px] leading-7 text-gray-900 outline-none placeholder:text-gray-400"
        aria-label="Meeting notes"
        autoFocus
      />
    </div>
  );
}
