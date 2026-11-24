import { invoke } from "@tauri-apps/api/core";

const kana = /[\p{Script=Hiragana}\p{Script=Katakana}]/u;
const hangul = /\p{Script=Hangul}/u;
const han = /\p{Script=Han}/u;

export function captionLanguage(text: string) {
  if (kana.test(text)) {
    return "ja";
  }

  if (hangul.test(text)) {
    return "ko";
  }

  if (han.test(text)) {
    return "zh";
  }

  return null;
}

interface Job {
  run: () => Promise<void>;
  live: boolean;
}

const queue: Job[] = [];
const captions = new Map<string, Promise<string>>();
let active = 0;
let resumeAt = 0;
let wakeup: ReturnType<typeof setTimeout> | null = null;
const completed = new Set<string>();

function drain() {
  if (active >= 2 || !queue.length) {
    return;
  }

  const delay = resumeAt - Date.now();

  if (delay > 0) {
    wakeup ??= setTimeout(() => {
      wakeup = null;
      drain();
    }, delay);

    return;
  }

  const nextLive = queue.findIndex((job) => job.live);
  const [job] = queue.splice(Math.max(0, nextLive), 1);
  active += 1;
  void job.run().finally(() => {
    active -= 1;
    drain();
  });
}

export function englishCaption(
  text: string,
  live: boolean
): Promise<string> | null {
  const language = captionLanguage(text);

  if (!language) {
    return null;
  }

  const cached = captions.get(text);

  if (cached) {
    return cached;
  }

  const pending = new Promise<string>((resolve, reject) => {
    queue.push({
      live,
      run: async () => {
        try {
          const english = await invoke<string>("translate_caption", {
            language,
            text,
          });

          completed.add(text);

          if (completed.size > 512) {
            const oldest = completed.values().next().value;

            if (oldest !== undefined) {
              completed.delete(oldest);
              captions.delete(oldest);
            }
          }

          resolve(english);
        } catch (error) {
          resumeAt = Date.now() + 30_000;
          captions.delete(text);
          reject(error);
        }
      },
    });
  });

  captions.set(text, pending);
  drain();

  return pending;
}
