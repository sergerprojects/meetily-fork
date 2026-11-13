-- Migration: Add Google Calendar event metadata to meetings
-- Nullable JSON blob of the matched calendar event (summary, times, attendees, video link)

ALTER TABLE meetings ADD COLUMN calendar_event_json TEXT;
