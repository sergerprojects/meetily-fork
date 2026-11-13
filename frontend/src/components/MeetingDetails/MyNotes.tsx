'use client';

import { useEffect, useState } from 'react';
import { readMeetingNotes } from '@/lib/capture';

/** The notes the user typed during the meeting (notes.md), shown above the summary. */
export function MyNotes({ folder }: { folder: string | null | undefined }) {
  const [text, setText] = useState('');

  useEffect(() => {
    if (!folder) {
      setText('');
      return;
    }
    let active = true;
    readMeetingNotes(folder)
      .then(t => active && setText(t.trim()))
      .catch(() => active && setText(''));
    return () => {
      active = false;
    };
  }, [folder]);

  if (!text) return null;

  return (
    <section className="mb-6 rounded-lg border border-gray-200 bg-gray-50 p-4" aria-label="My notes">
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">My notes</h3>
      <p className="whitespace-pre-wrap text-[15px] leading-7 text-gray-900">{text}</p>
    </section>
  );
}
