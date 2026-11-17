import { KeyRound } from "lucide-react";

import { ActivationForm } from "@/components/settings/ActivationForm";
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
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[440px]">
        <DialogHeader>
          <DialogTitle className="flex items-center">
            <KeyRound className="h-5 w-5 text-info" />
            Activate Meetily
          </DialogTitle>
          <DialogDescription>
            Enter your eight-digit activation code to start recording and
            transcribing meetings.
          </DialogDescription>
        </DialogHeader>
        <ActivationForm onActivated={() => onOpenChange(false)} />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Later
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
