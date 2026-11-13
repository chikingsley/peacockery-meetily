import { KeyRound } from "lucide-react";
import { useRouter } from "next/navigation";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface VoiceNotConfiguredDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** Shown when a recording is started without a Peacockery Voice API key. */
export function VoiceNotConfiguredDialog({
  open,
  onOpenChange,
}: VoiceNotConfiguredDialogProps) {
  const router = useRouter();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[440px]">
        <DialogHeader>
          <DialogTitle className="flex items-center">
            <KeyRound className="h-5 w-5 text-info" />
            Peacockery Voice not configured
          </DialogTitle>
          <DialogDescription>
            Transcription and summaries run on the Peacockery Voice API. Add the
            API key in Settings first.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            onClick={() => {
              onOpenChange(false);
              router.push("/settings?tab=voice");
            }}
          >
            Open settings
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
