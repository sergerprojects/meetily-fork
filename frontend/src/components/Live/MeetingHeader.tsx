'use client';

import { useEffect, useRef, useState } from 'react';
import { Calendar as CalendarIcon, ChevronDown, Users, Video } from 'lucide-react';
import { CalendarEvent } from '@/lib/calendar';
import { openExternal } from '@/lib/chat';

function initials(name: string) {
  const p = name.trim().split(/\s+/).filter(Boolean);
  return ((p[0]?.[0] ?? '?') + (p.length > 1 ? p[p.length - 1][0] : '')).toUpperCase();
}

/** "Salesforce" for @salesforce.com; public mail domains become "Guests". */
function orgOf(email: string) {
  const domain = email.split('@')[1]?.toLowerCase() ?? '';
  if (!domain || /^(gmail|yahoo|outlook|hotmail|icloud|proton)\./.test(domain)) return 'Guests';
  const sld = domain.split('.').slice(-2)[0];
  return sld.charAt(0).toUpperCase() + sld.slice(1);
}

function plain(html: string | null) {
  if (!html) return '';
  return html.replace(/<[^>]*>/g, ' ').replace(/&nbsp;/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 180);
}

interface Props {
  title: string;
  onTitleChange: (v: string) => void;
  event: CalendarEvent | null;
}

/** Title, then chips (date, people). The chips open the calendar event: details, Join, attendees. */
export function MeetingHeader({ title, onTitleChange, event }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const start = event ? new Date(event.start) : null;
  const end = event ? new Date(event.end) : null;
  const fmtTime = (d: Date) => d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const dateLine = start && end
    ? `${start.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })} · ${fmtTime(start)} – ${fmtTime(end)}`
    : '';

  const groups: Record<string, CalendarEvent['attendees']> = {};
  for (const a of event?.attendees ?? []) (groups[orgOf(a.email)] ||= []).push(a);

  const chip = 'inline-flex items-center gap-1.5 rounded-full border border-gray-200 px-2.5 py-1 text-xs text-gray-700 hover:bg-gray-50';

  return (
    <div className="relative" ref={ref}>
      <input
        value={title}
        onChange={e => onTitleChange(e.target.value)}
        className="w-full bg-transparent font-serif text-[28px] leading-tight text-gray-900 outline-none placeholder:text-gray-300"
        placeholder="Untitled meeting"
        aria-label="Meeting title"
      />
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <button className={chip} onClick={() => event && setOpen(o => !o)} aria-expanded={open}>
          <CalendarIcon className="h-3.5 w-3.5" />
          Today
        </button>
        {event && event.attendees.length > 0 && (
          <button className={chip} onClick={() => setOpen(o => !o)} aria-expanded={open}>
            <Users className="h-3.5 w-3.5" />
            {event.attendees.length}
          </button>
        )}
      </div>

      {open && event && (
        <div className="absolute left-0 top-full z-30 mt-2 w-[420px] rounded-2xl border border-gray-200 bg-white p-4 shadow-xl">
          <div className="flex items-start gap-3">
            <span className="mt-1.5 h-3 w-3 shrink-0 rounded-sm bg-rose-900" />
            <div className="min-w-0 flex-1">
              <div className="flex items-center justify-between gap-2 font-medium text-gray-900">
                <span className="truncate">{event.summary}</span>
                <ChevronDown className="h-4 w-4 shrink-0 text-gray-400" />
              </div>
              <div className="text-xs text-gray-500">{dateLine}</div>
            </div>
          </div>
          {plain(event.description) && (
            <p className="mt-3 text-sm leading-5 text-gray-600">{plain(event.description)}…</p>
          )}
          <div className="mt-3 flex gap-2">
            {event.video_url && (
              <button
                onClick={() => openExternal(event.video_url!)}
                className="flex flex-1 items-center justify-center gap-2 rounded-full bg-gray-100 px-3 py-2 text-sm text-gray-900 hover:bg-gray-200"
              >
                <Video className="h-4 w-4" /> Join
              </button>
            )}
            {event.html_link && (
              <button
                onClick={() => openExternal(event.html_link!)}
                className="flex flex-1 items-center justify-center gap-2 rounded-full border border-gray-200 px-3 py-2 text-sm text-gray-900 hover:bg-gray-50"
              >
                <CalendarIcon className="h-4 w-4" /> Calendar
              </button>
            )}
          </div>
          <div className="mt-3 max-h-64 overflow-y-auto border-t border-gray-100 pt-2">
            {Object.entries(groups).map(([org, people]) => (
              <div key={org} className="mb-2">
                <div className="px-1 pb-1 text-xs font-medium text-gray-400">{org}</div>
                {people.map(a => (
                  <div key={a.email} className="flex items-center gap-3 rounded-lg px-1 py-1.5">
                    <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-gray-200 text-[11px] font-medium text-gray-600">
                      {initials(a.name)}
                    </span>
                    <span className="truncate text-sm text-gray-900">
                      {a.name}
                      {a.is_self && <span className="ml-1 text-gray-400">(you)</span>}
                    </span>
                  </div>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
