import fs from "node:fs";
const [version, signatureFile, destination] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error("A stable semantic version is required");
const signature = fs.readFileSync(signatureFile, "utf8").trim();
if (!signature) throw new Error("The update signature is required");
const notes = fs.readFileSync(new URL("../RELEASE-NOTES.md", import.meta.url), "utf8");
fs.writeFileSync(destination, JSON.stringify({ version, notes, pub_date: new Date().toISOString(), platforms: { "windows-x86_64": { signature, url: `https://github.com/chikingsley/peacockery-meetily/releases/download/v${version}/Peacockery-Meetily-Setup.exe` } } }, null, 2));
