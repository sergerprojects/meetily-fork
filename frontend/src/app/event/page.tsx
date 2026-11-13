"use client";

import { Suspense, useEffect, useMemo, useState } from 'react';
import { useRouter, useSearchParams, usePathname } from 'next/navigation';
import { ArrowLeft, Calendar as CalendarIcon, Mic, Video } from 'lucide-react';
import { CalendarEvent, HomeMeeting, getCalendarWeek, getHomeMeetings, startRecordingForEvent } from '@/lib/calendar';
import { openExternal } from '@/lib/chat';

function plain(html: string | null) {
  if (!html) return '';
  return html.replace(/<[^>]*>/g, ' ').replace(/&nbsp;/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 400);
}

function EventPage() {
  const router = useRouter();
  const pathname = usePathname();
  const id = useSearchParams().get('id') ?? '';
  const [event, setEvent] = useState<CalendarEvent | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [recordings, setRecordings] = useState<HomeMeeting[]>([]);

  useEffect(() => {
    let active = true;
    (async () => {
      const [week, saved] = await Promise.all([getCalendarWeek(), getHomeMeetings(300)]);
      if (!active) return;
      setEvent(week.events.find(e => e.id === id) ?? null);
      setRecordings(saved.filter(m => m.event_id === id).sort((a, b) => +new Date(b.when) - +new Date(a.when)));
      setLoaded(true);
    })();
    return () => {
      active = false;
    };
  }, [id]);

  const start = useMemo(() => new Date(event?.start ?? 0), [event]);
  const end = useMemo(() => new Date(event?.end ?? 0), [event]);
  const fmt = (d: Date) => d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const past = event ? +end <= Date.now() : false;

  if (!loaded) return <div className="flex-1 p-10 text-sm text-gray-500">Loading…</div>;
  if (!event) {
    return (
      <div className="flex-1 p-10">
        <button className="mb-4 inline-flex items-center gap-1 text-sm text-gray-600 hover:text-gray-900" onClick={() => router.push('/')}>
          <ArrowLeft className="h-4 w-4" /> Home
        </button>
        <p className="text-sm text-gray-600">That meeting is no longer on your calendar view.</p>
      </div>
    );
  }

  const others = event.attendees.filter(a => !a.is_self);
  return (
    <div className="min-h-screen flex-1 overflow-y-auto bg-[#f7f6f1] pb-32">
      <div className="mx-auto max-w-[680px] px-6 pt-8">
        <button className="mb-6 inline-flex items-center gap-1 text-sm text-gray-600 hover:text-gray-900" onClick={() => router.push('/')}>
          <ArrowLeft className="h-4 w-4" /> Home
        </button>
        <h1 className="font-display text-[28px] font-semibold leading-tight tracking-[-0.005em] text-gray-900">{event.summary}</h1>
        <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-gray-600">
          <span className="inline-flex items-center gap-1.5 rounded-full border border-gray-200 px-2.5 py-1">
            <CalendarIcon className="h-3.5 w-3.5" />
            {start.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })} · {fmt(start)} – {fmt(end)}
          </span>
          {past && <span className="text-gray-400">Already happened</span>}
        </div>

        <div className="mt-6 flex flex-wrap gap-2">
          <button
            className="inline-flex items-center gap-2 rounded-full bg-gray-900 px-5 py-2.5 text-sm font-medium text-white hover:bg-gray-700"
            onClick={() => startRecordingForEvent(event, pathname, () => router.push('/'))}
          >
            <Mic className="h-4 w-4" />
            {recordings.length > 0 ? 'Continue recording' : 'Start recording'}
          </button>
          {event.video_url && (
            <button className="inline-flex items-center gap-2 rounded-full border border-gray-200 bg-white px-4 py-2.5 text-sm text-gray-900 hover:bg-gray-50" onClick={() => openExternal(event.video_url!)}>
              <Video className="h-4 w-4" /> Join
            </button>
          )}
          {event.html_link && (
            <button className="inline-flex items-center gap-2 rounded-full border border-gray-200 bg-white px-4 py-2.5 text-sm text-gray-900 hover:bg-gray-50" onClick={() => openExternal(event.html_link!)}>
              <CalendarIcon className="h-4 w-4" /> Calendar
            </button>
          )}
        </div>
        <p className="mt-2 text-xs text-gray-500">You can start recording at any time, even before or after the scheduled slot.</p>

        {recordings.length > 0 ? (
          <section className="mt-8">
            <h2 className="mb-2 text-sm font-medium text-gray-700">Notes from this meeting</h2>
            <ul className="overflow-hidden rounded-2xl border border-gray-200 bg-white">
              {recordings.map((r, i) => (
                <li key={r.id} className={i > 0 ? 'border-t border-gray-100' : ''}>
                  <button className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-gray-50" onClick={() => router.push(`/meeting-details?id=${r.id}`)}>
                    <span className="truncate text-[15px] text-gray-900">{r.title}</span>
                    <span className="shrink-0 text-xs text-gray-500">{new Date(r.when).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ) : (
          <section className="mt-8 rounded-2xl border border-dashed border-gray-300 p-6 text-sm text-gray-500">
            {past ? 'No notes were recorded for this meeting.' : 'Nothing recorded yet. Start recording when the meeting begins, or now.'}
          </section>
        )}

        {plain(event.description) && (
          <section className="mt-8">
            <h2 className="mb-2 text-sm font-medium text-gray-700">Description</h2>
            <p className="text-sm leading-6 text-gray-600">{plain(event.description)}</p>
          </section>
        )}

        {others.length > 0 && (
          <section className="mt-8">
            <h2 className="mb-2 text-sm font-medium text-gray-700">{others.length + 1} people</h2>
            <ul className="grid grid-cols-1 gap-x-6 sm:grid-cols-2">
              {event.attendees.map(a => (
                <li key={a.email} className="truncate py-1 text-sm text-gray-900">
                  {a.name}
                  {a.is_self && <span className="ml-1 text-gray-400">(you)</span>}
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}

export default function Page() {
  return (
    <Suspense fallback={null}>
      <EventPage />
    </Suspense>
  );
}
