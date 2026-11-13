/** Chip colour of the local speaker ("Me"); never used for anyone else. */
const ME_CLASS = "bg-blue-100 text-blue-800";

/** Colours for every other speaker, in the order numbered speakers get them. */
const OTHER_CLASSES = [
  "bg-emerald-100 text-emerald-800",
  "bg-amber-100 text-amber-800",
  "bg-purple-100 text-purple-800",
  "bg-rose-100 text-rose-800",
  "bg-cyan-100 text-cyan-800",
  "bg-lime-100 text-lime-800",
];

function hash(text: string): number {
  let h = 0;

  for (let i = 0; i < text.length; i++) {
    h = (h * 31 + text.charCodeAt(i)) | 0;
  }

  return Math.abs(h);
}

/**
 * Stable Tailwind chip colours per speaker label. The `meLabel` speaker is always blue and no
 * other speaker is; numbered speakers ("Them 2", "Speaker 3") take consecutive colours so the
 * people on one call stay distinct; any other name hashes to a stable colour.
 */
export function speakerColorClass(speaker: string, meLabel = "Me"): string {
  const name = speaker.trim();

  if (name === meLabel.trim()) {
    return ME_CLASS;
  }

  const numbered = /^(.*\S)\s+(\d+)$/.exec(name);

  if (numbered) {
    return OTHER_CLASSES[
      (Number(numbered[2]) - 1 + OTHER_CLASSES.length) % OTHER_CLASSES.length
    ];
  }

  return OTHER_CLASSES[hash(name) % OTHER_CLASSES.length];
}
