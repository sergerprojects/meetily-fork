'use client';

import { useEffect, useRef, useState } from 'react';
import { ArrowUp, X } from 'lucide-react';
import { askMeeting, ChatModel, ChatTurn } from '@/lib/chat';

const QUICK_ASKS: { label: string; question: string }[] = [
  { label: 'What did I miss', question: 'What did I miss? Summarize the last few minutes in 3 bullets.' },
  { label: 'Make me sound smart', question: 'Give me one sharp, well-grounded comment I could make right now, based on what has been said. One or two sentences.' },
  { label: 'Suggest questions', question: 'Suggest 3 good questions I could ask next, based on what has been said.' },
  { label: 'Action items so far', question: 'List the action items and commitments mentioned so far, with who owns each.' },
];

const MODELS: { value: ChatModel; label: string }[] = [
  { value: 'haiku', label: 'Fast' },
  { value: 'sonnet', label: 'Balanced' },
  { value: 'opus', label: 'Deep' },
];

interface Props {
  /** Latest transcript as plain text, with speaker names. */
  getTranscript: () => string;
  notes: string;
  title: string;
  attendees: string[];
}

/** Chat with the meeting while it happens. Answers come from your own Claude Code login. */
export function LiveChat({ getTranscript, notes, title, attendees }: Props) {
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [model, setModel] = useState<ChatModel>('sonnet');
  const [open, setOpen] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [turns, busy]);

  const ask = async (question: string) => {
    const q = question.trim();
    if (!q || busy) return;
    setOpen(true);
    setError(null);
    setInput('');
    const history = turns;
    setTurns([...history, { role: 'user', text: q }]);
    setBusy(true);
    try {
      const answer = await askMeeting({ question: q, transcript: getTranscript(), notes, title, attendees, history, model });
      setTurns(t => [...t, { role: 'assistant', text: answer }]);
    } catch (e) {
      setError(typeof e === 'string' ? e : 'Something went wrong. Try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="sticky bottom-24 z-20 flex justify-center px-4">
      <div className="w-2/3 max-w-[750px]">
        {open && (turns.length > 0 || busy || error) && (
          <div className="mb-2 max-h-[38vh] overflow-y-auto rounded-2xl border border-gray-200 bg-white p-4 shadow-lg">
            <div className="mb-2 flex justify-end">
              <button
                className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700"
                onClick={() => setOpen(false)}
                aria-label="Hide chat"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="space-y-3 text-[15px] leading-6">
              {turns.map((t, i) => (
                <div key={i} className={t.role === 'user' ? 'text-gray-500' : 'whitespace-pre-wrap text-gray-900'}>
                  {t.text}
                </div>
              ))}
              {busy && <div className="animate-pulse text-gray-400">Thinking…</div>}
              {error && <div className="text-sm text-red-600">{error}</div>}
              <div ref={endRef} />
            </div>
          </div>
        )}

        <div className="rounded-2xl border border-gray-200 bg-white/95 p-2 shadow-lg backdrop-blur">
          <div className="mb-1.5 flex flex-wrap gap-1.5">
            {QUICK_ASKS.map(q => (
              <button
                key={q.label}
                onClick={() => ask(q.question)}
                disabled={busy}
                className="rounded-full border border-gray-200 px-2.5 py-1 text-xs text-gray-700 hover:bg-gray-50 disabled:opacity-50"
              >
                {q.label}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-2">
            <input
              value={input}
              onChange={e => setInput(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && !e.shiftKey && ask(input)}
              placeholder="Ask about this meeting…"
              className="min-w-0 flex-1 bg-transparent px-2 py-1.5 text-[15px] text-gray-900 outline-none placeholder:text-gray-400"
              aria-label="Ask about this meeting"
            />
            <select
              value={model}
              onChange={e => setModel(e.target.value as ChatModel)}
              className="rounded-md bg-transparent px-1 py-1 text-xs text-gray-500 outline-none hover:bg-gray-100"
              aria-label="Answer speed"
            >
              {MODELS.map(m => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
            <button
              onClick={() => ask(input)}
              disabled={busy || !input.trim()}
              className="flex h-8 w-8 items-center justify-center rounded-full bg-gray-900 text-white disabled:opacity-30"
              aria-label="Send"
            >
              <ArrowUp className="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
