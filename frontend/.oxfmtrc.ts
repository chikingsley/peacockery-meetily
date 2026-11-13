import preset from "ultracite/oxfmt";

export default {
  ...preset,
  ignorePatterns: [
    ...(preset.ignorePatterns ?? []),
    "src-tauri/**",
    "pnpm-lock.yaml",
    "next-env.d.ts",
  ],
  sortTailwindcss: {
    ...preset.sortTailwindcss,
    stylesheet: "./src/app/globals.css",
  },
};
