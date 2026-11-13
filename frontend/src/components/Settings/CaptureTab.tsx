"use client";

import { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { CaptureSettings } from '@/components/Calendar/CaptureSettings';
import { CaptureDiagnostics } from '@/components/Calendar/CaptureSettings';

/** What Graphite reads from the call app: participant names and shared screens. */
export function CaptureTab() {
  const [showAdvanced, setShowAdvanced] = useState(false);
  return (
    <div className="space-y-4">
      <CaptureSettings />
      <div>
        <button
          className="flex items-center gap-1 text-sm text-gray-600 hover:text-gray-900"
          onClick={() => setShowAdvanced(v => !v)}
          aria-expanded={showAdvanced}
        >
          {showAdvanced ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
          Advanced
        </button>
        {showAdvanced && (
          <div className="mt-3">
            <CaptureDiagnostics />
          </div>
        )}
      </div>
    </div>
  );
}
