import { useCallback, useEffect, useState } from 'react';
import { voiceService } from '@/services/voiceService';

/** Whether Windows reports any capture device (mic) and any render/loopback source (system). */
export function useAudioDeviceCheck() {
  const [hasMicrophone, setHasMicrophone] = useState(true);
  const [hasSystemAudio, setHasSystemAudio] = useState(true);
  const [isChecking, setIsChecking] = useState(true);

  const check = useCallback(async () => {
    setIsChecking(true);
    try {
      const devices = await voiceService.getAudioDevices();
      setHasMicrophone(devices.some(d => d.device_type === 'Input'));
      setHasSystemAudio(devices.some(d => d.device_type === 'Output'));
    } catch (error) {
      console.error('get_audio_devices failed:', error);
      setHasMicrophone(false);
      setHasSystemAudio(false);
    } finally {
      setIsChecking(false);
    }
  }, []);

  useEffect(() => { void check(); }, [check]);

  return { hasMicrophone, hasSystemAudio, isChecking, check };
}
