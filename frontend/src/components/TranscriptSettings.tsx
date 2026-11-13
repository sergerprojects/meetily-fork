import { Switch } from './ui/switch';
import { ParakeetModelManager } from './ParakeetModelManager';
import { useConfig } from '@/contexts/ConfigContext';

export interface TranscriptModelProps {
    provider: 'localWhisper' | 'parakeet' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
    model: string;
    apiKey?: string | null;
}

export interface TranscriptSettingsProps {
    transcriptModelConfig: TranscriptModelProps;
    setTranscriptModelConfig: (config: TranscriptModelProps) => void;
    onModelSelect?: () => void;
}

/**
 * Speech recognition. Graphite transcribes on this Mac with Parakeet, which detects
 * the language by itself, so there is one model to download and nothing else to pick.
 */
export function TranscriptSettings({ transcriptModelConfig, setTranscriptModelConfig, onModelSelect }: TranscriptSettingsProps) {
    const { showConfidenceIndicator, toggleConfidenceIndicator } = useConfig();

    const handleParakeetModelSelect = (modelName: string) => {
        setTranscriptModelConfig({
            ...transcriptModelConfig,
            provider: 'parakeet',
            model: modelName
        });
        if (onModelSelect) {
            onModelSelect();
        }
    };

    return (
        <div className="space-y-6">
            <div>
                <h3 className="text-lg font-semibold text-gray-900">Transcription</h3>
                <p className="text-sm text-gray-600">
                    Speech is turned into text on this Mac. No audio leaves it, and the language is detected automatically.
                </p>
            </div>

            <ParakeetModelManager
                selectedModel={transcriptModelConfig.provider === 'parakeet' ? transcriptModelConfig.model : undefined}
                onModelSelect={handleParakeetModelSelect}
                autoSave={true}
            />

            <div className="flex items-center justify-between rounded-lg border p-4">
                <div className="flex-1 pr-4">
                    <div className="font-medium">Show how sure the transcription is</div>
                    <div className="text-sm text-gray-600">Hover a timestamp in a transcript to see a confidence score.</div>
                </div>
                <Switch checked={showConfidenceIndicator} onCheckedChange={toggleConfidenceIndicator} />
            </div>
        </div>
    )
}
