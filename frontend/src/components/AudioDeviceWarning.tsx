import { AlertTriangle, RefreshCw } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

interface AudioDeviceWarningProps {
  hasMicrophone: boolean;
  hasSystemAudio: boolean;
  onRecheck: () => void;
  isRechecking: boolean;
}

export function AudioDeviceWarning({
  hasMicrophone,
  hasSystemAudio,
  onRecheck,
  isRechecking,
}: AudioDeviceWarningProps) {
  if (hasMicrophone && hasSystemAudio) {
    return null;
  }

  return (
    <Alert className="mb-4 max-w-md">
      <AlertTriangle className="h-5 w-5 text-warning" />
      <AlertTitle>
        {hasMicrophone ? "No system audio source found" : "No microphone found"}
      </AlertTitle>
      <AlertDescription className="mt-2">
        {!hasMicrophone && (
          <p>
            Windows reports no capture device. Turn on and connect the Jabra
            headset (or another mic), then check Settings &gt; System &gt; Sound
            &gt; Input.
          </p>
        )}
        {!hasSystemAudio && (
          <p>
            Windows reports no playback device, so call audio cannot be
            captured. You can still record the microphone.
          </p>
        )}
        <button
          type="button"
          onClick={onRecheck}
          disabled={isRechecking}
          className="inline-flex items-center gap-2 rounded-md bg-warning/10 px-3 py-1.5 text-sm font-medium text-warning hover:bg-warning/10 disabled:opacity-50"
        >
          <RefreshCw
            className={`h-4 w-4 ${isRechecking ? "animate-spin" : ""}`}
          />
          Recheck
        </button>
      </AlertDescription>
    </Alert>
  );
}
