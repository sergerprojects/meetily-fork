"use client";

import { CalendarSettings } from '@/components/Calendar/CalendarSettings';

/** Which calendar Graphite reads, so meetings get titles and attendee names. */
export function CalendarTab() {
  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-lg font-semibold text-gray-900">Calendar</h3>
        <p className="text-sm text-gray-600">
          Graphite reads your calendar to title recordings, list who was invited, and offer to record when a meeting with a video link starts. It only reads; it never changes events.
        </p>
      </div>
      <CalendarSettings section="calendar" />
    </div>
  );
}
