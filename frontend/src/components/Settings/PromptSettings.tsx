'use client';

import { useEffect, useState } from 'react';
import { Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { SavedPrompt, loadPrompts, newPromptId, resetPrompts, savePrompts } from '@/lib/prompts';

/** Edit the quick questions shown above the meeting chat. */
export function PromptSettings() {
  const [list, setList] = useState<SavedPrompt[]>([]);

  useEffect(() => {
    setList(loadPrompts());
  }, []);

  const update = (next: SavedPrompt[]) => {
    setList(next);
    savePrompts(next);
  };

  return (
    <div className="space-y-3">
      <div>
        <div className="font-medium">Quick questions</div>
        <div className="text-sm text-gray-600">
          Buttons above the chat box while you record. Click one to ask Claude that question about the meeting so far.
        </div>
      </div>
      <ul className="space-y-2">
        {list.map(p => (
          <li key={p.id} className="flex items-start gap-2">
            <Input
              className="w-44 shrink-0"
              value={p.label}
              aria-label="Button label"
              onChange={e => update(list.map(x => (x.id === p.id ? { ...x, label: e.target.value } : x)))}
            />
            <Input
              value={p.question}
              aria-label="Question"
              onChange={e => update(list.map(x => (x.id === p.id ? { ...x, question: e.target.value } : x)))}
            />
            <button
              className="mt-2 rounded p-1 text-gray-400 hover:bg-red-50 hover:text-red-600"
              onClick={() => update(list.filter(x => x.id !== p.id))}
              aria-label={`Remove ${p.label}`}
            >
              <Trash2 className="h-4 w-4" />
            </button>
          </li>
        ))}
      </ul>
      <div className="flex gap-2">
        <Button
          size="sm"
          variant="outline"
          onClick={() => update([...list, { id: newPromptId(), label: 'New question', question: 'Ask something about the meeting' }])}
        >
          Add a question
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            resetPrompts();
            setList(loadPrompts());
          }}
        >
          Restore the originals
        </Button>
      </div>
    </div>
  );
}
