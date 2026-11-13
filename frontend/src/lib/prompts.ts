/** Saved questions for the meeting chat ("recipes"). Stored on this Mac only. */
export interface SavedPrompt {
  id: string;
  label: string;
  question: string;
}

const KEY = 'graphite.chatPrompts.v1';

export const DEFAULT_PROMPTS: SavedPrompt[] = [
  { id: 'missed', label: 'What did I miss', question: 'What did I miss? Summarize the last few minutes in 3 bullets.' },
  { id: 'smart', label: 'Make me sound smart', question: 'Give me one sharp, well-grounded comment I could make right now, based on what has been said. One or two sentences.' },
  { id: 'questions', label: 'Suggest questions', question: 'Suggest 3 good questions I could ask next, based on what has been said.' },
  { id: 'whoswho', label: "Who's who", question: "Match each 'Speaker N' to an invitee using only what has been said (names used, who is addressed, roles). For each speaker give the name and the evidence in a few words, or 'unsure'. Do not guess." },
  { id: 'actions', label: 'Action items so far', question: 'List the action items and commitments mentioned so far, with who owns each.' },
];

export function isValidPrompt(p: unknown): p is SavedPrompt {
  const x = p as SavedPrompt;
  return !!x && typeof x.id === 'string' && typeof x.label === 'string' && typeof x.question === 'string'
    && x.label.trim() !== '' && x.question.trim() !== '';
}

export function loadPrompts(): SavedPrompt[] {
  if (typeof window === 'undefined') return DEFAULT_PROMPTS;
  try {
    const raw = window.localStorage.getItem(KEY);
    if (!raw) return DEFAULT_PROMPTS;
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) && parsed.every(isValidPrompt) ? parsed : DEFAULT_PROMPTS;
  } catch {
    return DEFAULT_PROMPTS;
  }
}

export function savePrompts(list: SavedPrompt[]): void {
  window.localStorage.setItem(KEY, JSON.stringify(list.filter(isValidPrompt)));
  window.dispatchEvent(new Event('graphite-prompts-changed'));
}

export function resetPrompts(): void {
  window.localStorage.removeItem(KEY);
  window.dispatchEvent(new Event('graphite-prompts-changed'));
}

export function newPromptId(): string {
  return `p${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
}
