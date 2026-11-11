import { useState, useEffect, useCallback } from 'react';
import { toast } from 'sonner';
import { voiceService } from '@/services/voiceService';
import type { SummaryTemplate } from '@/types/voice';

export function useTemplates() {
  const [availableTemplates, setAvailableTemplates] = useState<SummaryTemplate[]>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<string>('standard_meeting');

  useEffect(() => {
    voiceService.listTemplates()
      .then(setAvailableTemplates)
      .catch((error) => console.error('Failed to fetch templates:', error));
  }, []);

  const handleTemplateSelection = useCallback((templateId: string, templateName: string) => {
    setSelectedTemplate(templateId);
    toast.success('Template selected', {
      description: `Using "${templateName}" template for summary generation`,
    });
  }, []);

  return { availableTemplates, selectedTemplate, handleTemplateSelection };
}
