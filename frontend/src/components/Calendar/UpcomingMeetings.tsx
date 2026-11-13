'use client';

import { useEffect, useState } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { Video } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  CalendarEvent,
  formatEventTime,
  getCalendarStatus,
  getUpcomingCalendarEvents,
  startRecordingForEvent,
} from '@/lib/calendar';

const MAX_ITEMS = 3;
const REFRESH_MS = 60 * 1000;

/** Next few video meetings from Google Calendar, each with a Record button. */
export function UpcomingMeetings() {
  const pathname = usePathname();
  const router = useRouter();
  const [enabled, setEnabled] = useState(false);
  const [events, setEvents] = useState<CalendarEvent[]>([]);

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setInterval> | undefined;

    const refresh = async () => {
      const upcoming = await getUpcomingCalendarEvents();
      if (active) setEvents(upcoming.filter(e => e.video_url).slice(0, MAX_ITEMS));
    };

    (async () => {
      const status = await getCalendarStatus();
      if (!active || !status.enabled) return;
      setEnabled(true);
      await refresh();
      timer = setInterval(refresh, REFRESH_MS);
    })();

    return () => {
      active = false;
      if (timer) clearInterval(timer);
    };
  }, []);

  if (!enabled || events.length === 0) return null;

  return (
    <div className="w-2/3 max-w-[750px] rounded-lg border border-gray-200 bg-white p-3">
      <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">Upcoming</div>
      <ul className="space-y-2">
        {events.map(event => (
          <li key={event.id} className="flex items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2">
              <Video className="h-4 w-4 shrink-0 text-gray-400" />
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-gray-900">{event.summary}</div>
                <div className="text-xs text-gray-500">
                  {formatEventTime(event)}
                  {event.attendees.length > 0 && ` · ${event.attendees.length} attendee${event.attendees.length === 1 ? '' : 's'}`}
                </div>
              </div>
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => startRecordingForEvent(event, pathname, () => router.push('/'))}
            >
              Record
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}
