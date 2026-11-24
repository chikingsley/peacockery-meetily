import { useEffect, useState } from "react";

import { englishCaption } from "@/lib/captions";

export function EnglishCaption({
  text,
  live = false,
}: {
  text: string;
  live?: boolean;
}) {
  const [line, setLine] = useState<{ source: string; english: string } | null>(
    null
  );

  useEffect(() => {
    let mounted = true;
    let retry: ReturnType<typeof setTimeout>;

    function translate() {
      void englishCaption(text, live)
        ?.then((english) => {
          if (mounted) {
            setLine({ source: text, english });
          }
        })
        .catch(() => {
          if (mounted) {
            setLine({ source: text, english: "Translation delayed" });
            retry = setTimeout(translate, 30_000);
          }
        });
    }

    translate();

    return () => {
      mounted = false;
      clearTimeout(retry);
    };
  }, [text, live]);

  if (line?.source !== text) {
    return null;
  }

  return (
    <span
      lang="en"
      className="block text-sm leading-relaxed text-muted-foreground"
    >
      {line.english}
    </span>
  );
}
