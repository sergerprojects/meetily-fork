import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { PermissionWarning } from '@/components/PermissionWarning';
import { UpcomingMeetings } from '@/components/Calendar/UpcomingMeetings';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { ChevronUp, Copy, GlobeIcon, Minus } from 'lucide-react';
import { MeetingHeader } from '@/components/Live/MeetingHeader';
import { LiveChat } from '@/components/Live/LiveChat';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { ModalType } from '@/hooks/useModalState';
import { useIsLinux } from '@/hooks/usePlatform';
import { ReactNode, useEffect, useMemo, useState } from 'react';
import { getCurrentOtherName, getCurrentEvent, getCalendarAgenda, attachRecordingToEvent, saveLiveNotes, saveSpeakerNames, CalendarEvent } from '@/lib/calendar';

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
  /** Recording controls (pause / stop); shown in the bottom dock while recording. */
  dock?: ReactNode;
}

export function TranscriptPanel({
  isProcessingStop,
  isStopping,
  showModal,
  dock
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
  const [choices, setChoices] = useState<CalendarEvent[]>([]);
  const [notes, setNotes] = useState('');
  const [showTranscript, setShowTranscript] = useState(false);
  const [speakerNames, setSpeakerNames] = useState<Record<string, string>>({});
  const { isCollapsed: sidebarCollapsed } = useSidebar();
  const [wasRecording, setWasRecording] = useState(false);

  useEffect(() => {
    if (isRecording && !wasRecording) {
      setNotes('');
      setShowTranscript(false);
      setSpeakerNames({});
    }
    setWasRecording(isRecording);
  }, [isRecording, wasRecording]);

  useEffect(() => {
    if (!isRecording) return;
    let active = true;
    const refresh = async () => {
      const [e, agenda] = await Promise.all([getCurrentEvent(), getCalendarAgenda()]);
      if (!active) return;
      setEvent(e);
      const now = Date.now();
      setChoices(
        agenda.filter(a => a.id !== e?.id && new Date(a.start).getTime() <= now + 5 * 60_000 && new Date(a.end).getTime() > now)
      );
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

  // Keep the "Speaker N = person" choices with the recording.
  useEffect(() => {
    if (!isRecording || Object.keys(speakerNames).length === 0) return;
    saveSpeakerNames(speakerNames).catch(() => {});
  }, [speakerNames, isRecording]);

  const isDefaultTitle = /^(Meeting \d|\+ New Call)/.test(meetingTitle || '');
  const displayTitle = event && isDefaultTitle ? event.summary : meetingTitle;
  const others = (event?.attendees ?? []).filter(a => !a.is_self);
  // Voices the live labeler has told apart so far ("Speaker 1", "Speaker 2", ...).
  const speakerLabels = useMemo(
    () => Array.from(new Set(transcripts.map(t => t.speaker || '').filter(s => /^Speaker \d+$/.test(s)))).sort((a, b) => Number(a.slice(8)) - Number(b.slice(8))),
    [transcripts]
  );
  const showLiveView = isRecording || notes.trim().length > 0;

  // Convert transcripts to segments for virtualized view
  const segments = useMemo(() =>
    transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: otherName && t.speaker === 'Others' ? otherName : (t.speaker && speakerNames[t.speaker]) || t.speaker,
    })),
    [transcripts, otherName, speakerNames]
  );

  return (
    <div ref={transcriptContainerRef} className={`w-full border-r border-gray-200 flex flex-col overflow-y-auto ${showLiveView ? 'bg-[#f7f6f1]' : 'bg-white'}`}>
      {/* Title area - Sticky header */}
      {!showLiveView && <div className="sticky top-0 z-10 bg-white p-4 border-gray-200">
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
      </div>}

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

      {/* Live meeting: title + chips, then notes (the transcript runs in the background) */}
      {showLiveView && (
        <>
          <div className="flex justify-center px-4 pt-8">
            <div className="w-2/3 max-w-[750px]">
              <MeetingHeader
                title={displayTitle}
                onTitleChange={setMeetingTitle}
                event={event}
                choices={choices}
                onPick={async (picked) => {
                  await attachRecordingToEvent(picked);
                  setEvent(picked);
                  setMeetingTitle(picked.summary);
                }}
              />
            </div>
          </div>
          <div className="flex justify-center px-4 pt-4 pb-48">
            <div className="w-2/3 max-w-[750px]">
              <textarea
                value={notes}
                onChange={e => setNotes(e.target.value)}
                placeholder="Write notes. The transcript is being captured in the background."
                className="min-h-[55vh] w-full resize-none bg-transparent text-[15px] leading-7 text-gray-900 outline-none placeholder:text-gray-400"
                aria-label="Meeting notes"
                autoFocus
              />
            </div>
          </div>
        </>
      )}

      {/* Transcript content (not during a live meeting: it lives in the floating panel) */}
      {!showLiveView && (
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

      {/* Bottom dock: transcript toggle + recording controls + chat */}
      {isRecording && (
        <div className="pointer-events-none fixed bottom-6 left-0 right-0 z-30">
          <div className="flex justify-center transition-[margin] duration-300" style={{ marginLeft: sidebarCollapsed ? '4rem' : '16rem' }}>
            <div className="pointer-events-auto relative w-2/3 max-w-[750px]">
              {showTranscript && (
                <div className="absolute bottom-full left-0 right-0 mb-3 flex h-[55vh] flex-col overflow-hidden rounded-3xl border border-gray-200 bg-white shadow-2xl">
                  <div className="flex items-center justify-between border-b border-gray-100 px-4 py-2.5">
                    <span className="text-sm font-medium text-gray-700">Transcript</span>
                    <div className="flex items-center gap-1">
                      <button
                        onClick={copyTranscript}
                        className="rounded-md p-1.5 text-gray-500 hover:bg-gray-100"
                        title="Copy transcript"
                        aria-label="Copy transcript"
                      >
                        <Copy className="h-4 w-4" />
                      </button>
                      <button
                        onClick={() => setShowTranscript(false)}
                        className="rounded-md p-1.5 text-gray-500 hover:bg-gray-100"
                        title="Hide transcript"
                        aria-label="Hide transcript"
                      >
                        <Minus className="h-4 w-4" />
                      </button>
                    </div>
                  </div>
                  {speakerLabels.length > 0 && (
                    <div className="flex flex-wrap items-center gap-2 border-b border-gray-100 px-5 py-2 text-xs text-gray-500">
                      <span className="font-medium text-gray-600">Who's who</span>
                      {speakerLabels.map(label => (
                        <label key={label} className="inline-flex items-center gap-1.5">
                          <span>{label}</span>
                          {others.length > 0 ? (
                            <select
                              value={speakerNames[label] ?? ''}
                              onChange={e => setSpeakerNames(m => ({ ...m, [label]: e.target.value }))}
                              className="max-w-[160px] rounded-md border border-gray-200 bg-white px-1.5 py-0.5 text-xs text-gray-800"
                              aria-label={`Who is ${label}`}
                            >
                              <option value="">Not sure</option>
                              {others.map(a => (
                                <option key={a.email} value={a.name}>
                                  {a.name}
                                </option>
                              ))}
                            </select>
                          ) : (
                            <input
                              value={speakerNames[label] ?? ''}
                              onChange={e => setSpeakerNames(m => ({ ...m, [label]: e.target.value }))}
                              placeholder="Name"
                              className="w-28 rounded-md border border-gray-200 bg-white px-1.5 py-0.5 text-xs text-gray-800"
                              aria-label={`Who is ${label}`}
                            />
                          )}
                        </label>
                      ))}
                    </div>
                  )}
                  <div className="min-h-0 flex-1 px-5 py-3">
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
              )}
              <div className="flex items-end gap-3">
                <div className="flex shrink-0 items-center rounded-full border border-gray-200 bg-white shadow-lg">
                  <button
                    onClick={() => setShowTranscript(v => !v)}
                    className="ml-1.5 flex h-9 w-9 items-center justify-center rounded-full text-gray-600 hover:bg-gray-100"
                    aria-label={showTranscript ? 'Hide transcript' : 'Show transcript'}
                    aria-expanded={showTranscript}
                    title="Transcript"
                  >
                    <ChevronUp className={`h-4 w-4 transition-transform ${showTranscript ? 'rotate-180' : ''}`} />
                  </button>
                  {dock}
                </div>
                <LiveChat
                  getTranscript={() =>
                    segments
                      .map(seg => {
                        const t = Math.max(0, Math.floor(seg.timestamp));
                        const mm = String(Math.floor(t / 60)).padStart(2, '0');
                        const ss = String(t % 60).padStart(2, '0');
                        return `[${mm}:${ss}] ${seg.speaker ? seg.speaker + ': ' : ''}${seg.text}`;
                      })
                      .join('\n')
                  }
                  notes={notes}
                  title={displayTitle}
                  attendees={others.map(a => a.name)}
                />
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
