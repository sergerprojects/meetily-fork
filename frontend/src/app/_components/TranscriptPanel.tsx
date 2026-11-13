import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { PermissionWarning } from '@/components/PermissionWarning';
import { UpcomingMeetings } from '@/components/Calendar/UpcomingMeetings';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, GlobeIcon, Users, Calendar as CalendarIcon } from 'lucide-react';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { ModalType } from '@/hooks/useModalState';
import { useIsLinux } from '@/hooks/usePlatform';
import { useEffect, useMemo, useState } from 'react';
import { getCurrentOtherName, getCurrentEvent, saveLiveNotes, CalendarEvent } from '@/lib/calendar';

/**
 * TranscriptPanel Component
 *
 * Displays transcript content with controls for copying and language settings.
 * Uses TranscriptContext, ConfigContext, and RecordingStateContext internally.
 */

interface TranscriptPanelProps {
  // indicates stop-processing state for transcripts; derived from backend statuses.
  isProcessingStop: boolean;
  isStopping: boolean;
  showModal: (name: ModalType, message?: string) => void;
}

export function TranscriptPanel({
  isProcessingStop,
  isStopping,
  showModal
}: TranscriptPanelProps) {
  // Contexts
  const { transcripts, transcriptContainerRef, copyTranscript, meetingTitle, setMeetingTitle } = useTranscripts();
  const { transcriptModelConfig } = useConfig();
  const { isRecording, isPaused } = useRecordingState();
  const { checkPermissions, isChecking, hasSystemAudio, hasMicrophone } = usePermissionCheck();
  const isLinux = useIsLinux();

  // 1:1 on the calendar: show the other person by name instead of "Others".
  const [otherName, setOtherName] = useState<string | null>(null);
  useEffect(() => {
    if (!isRecording) {
      setOtherName(null);
      return;
    }
    let active = true;
    const refresh = async () => {
      const name = await getCurrentOtherName();
      if (active) setOtherName(name);
    };
    refresh();
    const timer = setInterval(refresh, 30_000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [isRecording]);

  // Header context (title, who is invited) + free-form notes for the live recording.
  const [event, setEvent] = useState<CalendarEvent | null>(null);
  const [notes, setNotes] = useState('');
  const [tab, setTab] = useState<'notes' | 'transcript'>('notes');
  const [wasRecording, setWasRecording] = useState(false);

  useEffect(() => {
    if (isRecording && !wasRecording) {
      setNotes('');
      setTab('notes');
    }
    setWasRecording(isRecording);
  }, [isRecording, wasRecording]);

  useEffect(() => {
    if (!isRecording) return;
    let active = true;
    const refresh = async () => {
      const e = await getCurrentEvent();
      if (active) setEvent(e);
    };
    refresh();
    const timer = setInterval(refresh, 30_000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [isRecording]);

  // Autosave notes next to the recording (notes.md) shortly after typing stops.
  useEffect(() => {
    if (!isRecording || notes === '') return;
    const t = setTimeout(() => {
      saveLiveNotes(notes).catch(() => {});
    }, 700);
    return () => clearTimeout(t);
  }, [notes, isRecording]);

  const isDefaultTitle = /^(Meeting \d|\+ New Call)/.test(meetingTitle || '');
  const displayTitle = event && isDefaultTitle ? event.summary : meetingTitle;
  const others = (event?.attendees ?? []).filter(a => !a.is_self);
  const showLiveView = isRecording || notes.trim().length > 0;

  // Convert transcripts to segments for virtualized view
  const segments = useMemo(() =>
    transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: otherName && t.speaker === 'Others' ? otherName : t.speaker,
    })),
    [transcripts, otherName]
  );

  return (
    <div ref={transcriptContainerRef} className="w-full border-r border-gray-200 bg-white flex flex-col overflow-y-auto">
      {/* Title area - Sticky header */}
      <div className="sticky top-0 z-10 bg-white p-4 border-gray-200">
        <div className="flex flex-col space-y-3">
          <div className="flex  flex-col space-y-2">
            <div className="flex justify-center  items-center space-x-2">
              <ButtonGroup>
                {transcripts?.length > 0 && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={copyTranscript}
                    title="Copy Transcript"
                  >
                    <Copy />
                    <span className='hidden md:inline'>
                      Copy
                    </span>
                  </Button>
                )}
                {transcriptModelConfig.provider === "localWhisper" &&
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => showModal('languageSettings')}
                    title="Language"
                  >
                    <GlobeIcon />
                    <span className='hidden md:inline'>
                      Language
                    </span>
                  </Button>
                }
              </ButtonGroup>
            </div>
          </div>
        </div>
      </div>

      {/* Permission Warning - Not needed on Linux */}
      {!isRecording && !isChecking && !isLinux && (
        <div className="flex justify-center px-4 pt-4">
          <PermissionWarning
            hasMicrophone={hasMicrophone}
            hasSystemAudio={hasSystemAudio}
            onRecheck={checkPermissions}
            isRechecking={isChecking}
          />
        </div>
      )}

      {/* Upcoming calendar meetings - only before a recording starts */}
      {!isRecording && transcripts.length === 0 && (
        <div className="flex justify-center px-4 pt-4">
          <UpcomingMeetings />
        </div>
      )}

      {/* Live meeting header: title + context chips (Granola-style) */}
      {showLiveView && (
        <div className="flex justify-center px-4 pt-2">
          <div className="w-2/3 max-w-[750px]">
            <input
              value={displayTitle}
              onChange={e => setMeetingTitle(e.target.value)}
              className="w-full bg-transparent font-serif text-[28px] leading-tight text-gray-900 outline-none placeholder:text-gray-300 focus-visible:ring-0"
              placeholder="Untitled meeting"
              aria-label="Meeting title"
            />
            <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-gray-600">
              <span className="inline-flex items-center gap-1.5 rounded-full border border-gray-200 px-2.5 py-1">
                <CalendarIcon className="h-3.5 w-3.5" />
                Today
              </span>
              {others.length > 0 && (
                <span
                  className="inline-flex items-center gap-1.5 rounded-full border border-gray-200 px-2.5 py-1"
                  title={others.map(a => a.name).join('\n')}
                >
                  <Users className="h-3.5 w-3.5" />
                  {others.length + 1}
                </span>
              )}
              {others.length > 0 && others.length <= 3 && (
                <span className="text-gray-500">{others.map(a => a.name).join(', ')}</span>
              )}
            </div>
            <div className="mt-5 inline-flex rounded-lg bg-gray-100 p-0.5 text-sm" role="tablist">
              {(['notes', 'transcript'] as const).map(t => (
                <button
                  key={t}
                  role="tab"
                  aria-selected={tab === t}
                  onClick={() => setTab(t)}
                  className={`rounded-md px-3 py-1 capitalize transition-colors ${
                    tab === t ? 'bg-white text-gray-900 shadow-sm' : 'text-gray-500 hover:text-gray-800'
                  }`}
                >
                  {t}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Free-form notes */}
      {showLiveView && tab === 'notes' && (
        <div className="flex justify-center px-4 pt-4 pb-32">
          <div className="w-2/3 max-w-[750px]">
            <textarea
              value={notes}
              onChange={e => setNotes(e.target.value)}
              placeholder="Write notes. They are saved with the meeting and merged into your summary."
              className="min-h-[50vh] w-full resize-none bg-transparent text-[15px] leading-7 text-gray-900 outline-none placeholder:text-gray-400"
              aria-label="Meeting notes"
              autoFocus
            />
          </div>
        </div>
      )}

      {/* Transcript content */}
      {(!showLiveView || tab === 'transcript') && (
        <div className="pb-20">
          <div className="flex justify-center">
            <div className="w-2/3 max-w-[750px]">
              <VirtualizedTranscriptView
                segments={segments}
                isRecording={isRecording}
                isPaused={isPaused}
                isProcessing={isProcessingStop}
                isStopping={isStopping}
                enableStreaming={isRecording}
                showConfidence={true}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
