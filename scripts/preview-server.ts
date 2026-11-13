import { resolve, sep } from 'node:path';
const root = resolve(import.meta.dir, '../frontend/out');
Bun.serve({
  hostname: '127.0.0.1',
  port: 3119,
  async fetch(request) {
    const url = new URL(request.url);
    let path: string;
    try { path = resolve(root, '.' + decodeURIComponent(url.pathname)); }
    catch { return new Response('Bad path', { status: 400 }); }
    if (path !== root && !path.startsWith(root + sep)) return new Response('Bad path', { status: 400 });
    const candidates = url.pathname === '/' ? [resolve(root, 'index.html')] : [path, path + '.html', resolve(path, 'index.html')];
    for (const candidate of candidates) {
      const file = Bun.file(candidate);
      if (await file.exists()) return new Response(file);
    }
    return new Response(Bun.file(resolve(root, '404.html')), { status: 404 });
  },
});
