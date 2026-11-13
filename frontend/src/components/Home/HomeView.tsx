'use client';

import { useEffect, useMemo, useState } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { Button } from '@/components/ui/button';
import {
  CalendarEvent,
  HomeMeeting,
  getCalendarAgenda,
  getCalendarStatus,
  getHomeMeetings,
  startRecordingForEvent,
} from '@/lib/calendar';

const REFRESH_MS = 60 * 1000;

const timeOf = (iso: string) =>
  new Date(iso).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

function dayKey(d: Date) {
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

function dayLabel(d: Date, now: Date) {
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  const tomorrow = new Date(now);
  tomorrow.setDate(now.getDate() + 1);
  if (dayKey(d) === dayKey(now)) return 'Today';
  if (dayKey(d) === dayKey(yesterday)) return 'Yesterday';
  if (dayKey(d) === dayKey(tomorrow)) return 'Tomorrow';
  return d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
}

function initials(name: string) {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  return ((parts[0][0] ?? '') + (parts.length > 1 ? parts[parts.length - 1][0] : '')).toUpperCase();
}

function whoLine(names: string[]) {
  if (names.length === 0) return 'Me';
  const shown = names.slice(0, 2).join(', ');
  const rest = names.length - 2;
  return rest > 0 ? `${shown} & ${rest} other${rest === 1 ? '' : 's'}` : names.join(' & ');
}

/** Granola-style home: "Coming up" from the calendar + saved meetings grouped by day. */
export function HomeView() {
  const router = useRouter();
  const pathname = usePathname();
  const [calendarOn, setCalendarOn] = useState(false);
  const [agenda, setAgenda] = useState<CalendarEvent[]>([]);
  const [meetings, setMeetings] = useState<HomeMeeting[]>([]);
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    let active = true;
    const refresh = async () => {
      const [status, events, saved] = await Promise.all([
        getCalendarStatus(),
        getCalendarAgenda(),
        getHomeMeetings(),
      ]);
      if (!active) return;
      setCalendarOn(status.enabled);
      setAgenda(events);
      setMeetings(saved);
      setNow(new Date());
    };
    refresh();
    const timer = setInterval(refresh, REFRESH_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  const agendaByDay = useMemo(() => {
    const groups: { label: string; events: CalendarEvent[] }[] = [];
    for (const e of agenda) {
      const label = dayLabel(new Date(e.start), now);
      const last = groups[groups.length - 1];
      if (last && last.label === label) last.events.push(e);
      else groups.push({ label, events: [e] });
    }
    return groups;
  }, [agenda, now]);

  const meetingsByDay = useMemo(() => {
    const groups: { label: string; items: HomeMeeting[] }[] = [];
    const sorted = [...meetings].sort((a, b) => +new Date(b.when) - +new Date(a.when));
    for (const m of sorted) {
      const label = dayLabel(new Date(m.when), now);
      const last = groups[groups.length - 1];
      if (last && last.label === label) last.items.push(m);
      else groups.push({ label, items: [m] });
    }
    return groups;
  }, [meetings, now]);

  const isNow = (e: CalendarEvent) => +new Date(e.start) <= +now && +new Date(e.end) > +now;

  return (
    <div className="w-full overflow-y-auto bg-[#f7f6f1] pb-40">
      <div className="mx-auto max-w-[640px] px-6 pt-10">
        {calendarOn && (
          <section className="mb-10">
            <h1 className="mb-4 font-serif text-3xl text-gray-900">Coming up</h1>
            <div className="rounded-2xl border border-gray-200 bg-white p-5 shadow-sm">
              {agendaByDay.length === 0 ? (
                <div className="text-sm text-gray-500">Nothing else on your calendar through tomorrow.</div>
              ) : (
                <div className="space-y-5">
                  {agendaByDay.map(group => (
                    <div key={group.label}>
                      <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-400">
                        {group.label}
                      </div>
                      <ul className="space-y-3">
                        {group.events.map(e => (
                          <li key={e.id} className="flex items-center justify-between gap-3 border-l-4 border-rose-900 pl-3">
                            <div className="min-w-0">
                              <div className="truncate text-[15px] font-medium text-gray-900">{e.summary}</div>
                              <div className="text-xs text-gray-500">
                                {isNow(e) && <span className="mr-1 font-semibold text-green-700">Now ·</span>}
                                {timeOf(e.start)} – {timeOf(e.end)}
                              </div>
                            </div>
                            {e.video_url && isNow(e) && (
                              <Button
                                size="sm"
                                className="shrink-0 rounded-full bg-gray-900 text-white hover:bg-gray-700"
                                onClick={() => startRecordingForEvent(e, pathname, () => router.push('/'))}
                              >
                                Start now
                              </Button>
                            )}
                          </li>
                        ))}
                      </ul>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </section>
        )}

        {meetingsByDay.length === 0 ? (
          <div className="py-10 text-center text-sm text-gray-500">
            No meetings yet. Press record or join a call and your notes will appear here.
          </div>
        ) : (
          meetingsByDay.map(group => (
            <section key={group.label} className="mb-8">
              <div className="mb-2 text-sm text-gray-500">{group.label}</div>
              <ul>
                {group.items.map(m => (
                  <li key={m.id}>
                    <button
                      className="flex w-full items-center gap-3 rounded-lg px-2 py-2.5 text-left hover:bg-white"
                      onClick={() => router.push(`/meeting-details?id=${m.id}`)}
                    >
                      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-gray-200 text-xs font-medium text-gray-600">
                        {initials(m.attendees[0] ?? 'Me')}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[15px] text-gray-900">{m.title}</div>
                        <div className="truncate text-xs text-gray-500">{whoLine(m.attendees)}</div>
                      </div>
                      <div className="shrink-0 text-xs text-gray-500">{timeOf(m.when)}</div>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))
        )}
      </div>
    </div>
  );
}
