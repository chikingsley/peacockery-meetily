import { AlertTriangle, RefreshCw } from 'lucide-react';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';

interface AudioDeviceWarningProps {
  hasMicrophone: boolean;
  hasSystemAudio: boolean;
  onRecheck: () => void;
  isRechecking: boolean;
}

export function AudioDeviceWarning({ hasMicrophone, hasSystemAudio, onRecheck, isRechecking }: AudioDeviceWarningProps) {
  if (hasMicrophone && hasSystemAudio) return null;

  return (
    <Alert className="max-w-md mb-4 border-amber-400 bg-amber-50">
      <AlertTriangle className="h-5 w-5 text-amber-600" />
      <AlertTitle className="text-amber-900 font-semibold">
        {!hasMicrophone ? 'No microphone found' : 'No system audio source found'}
      </AlertTitle>
      <AlertDescription className="text-amber-800 mt-2 space-y-2">
        {!hasMicrophone && (
          <p>
            Windows reports no capture device. Turn on and connect the Jabra headset (or another mic),
            then check Settings &gt; System &gt; Sound &gt; Input.
          </p>
        )}
        {!hasSystemAudio && (
          <p>
            Windows reports no playback device, so call audio cannot be captured. You can still record the microphone.
          </p>
        )}
        <button
          onClick={onRecheck}
          disabled={isRechecking}
          className="inline-flex items-center gap-2 px-3 py-1.5 text-sm font-medium text-amber-900 bg-amber-100 hover:bg-amber-200 rounded-md disabled:opacity-50"
        >
          <RefreshCw className={`h-4 w-4 ${isRechecking ? 'animate-spin' : ''}`} />
          Recheck
        </button>
      </AlertDescription>
    </Alert>
  );
}
