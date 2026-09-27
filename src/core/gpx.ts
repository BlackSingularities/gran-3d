import type { Route } from './graph';

const esc = (s: string) => s.replace(/[<>&"']/g, (c) => `&#${c.charCodeAt(0)};`);

export function routeToGpx(route: Route, name: string, waypoints: { name: string; lon: number; lat: number; e: number }[]) {
  const tr = route.track;
  const start = Date.now();
  const lines: string[] = [];
  lines.push('<?xml version="1.0" encoding="UTF-8"?>');
  lines.push(
    '<gpx version="1.1" creator="GRAŃ — atlas szlaków 3D" xmlns="http://www.topografix.com/GPX/1/1" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.topografix.com/GPX/1/1 http://www.topografix.com/GPX/1/1/gpx.xsd">'
  );
  lines.push(`  <metadata><name>${esc(name)}</name><time>${new Date(start).toISOString()}</time></metadata>`);
  for (const w of waypoints) {
    lines.push(`  <wpt lat="${w.lat.toFixed(6)}" lon="${w.lon.toFixed(6)}"><ele>${w.e.toFixed(1)}</ele><name>${esc(w.name)}</name></wpt>`);
  }
  lines.push(`  <trk><name>${esc(name)}</name><type>hiking</type><trkseg>`);
  for (let i = 0; i < tr.n; i++) {
    const t = new Date(start + tr.t[i] * 3600000).toISOString();
    lines.push(`    <trkpt lat="${tr.lat[i].toFixed(6)}" lon="${tr.lon[i].toFixed(6)}"><ele>${tr.e[i].toFixed(1)}</ele><time>${t}</time></trkpt>`);
  }
  lines.push('  </trkseg></trk>');
  lines.push('</gpx>');
  return lines.join('\n');
}

export interface GpxTrack {
  name: string;
  points: { lon: number; lat: number; e?: number }[];
}

export function parseGpx(text: string): GpxTrack {
  const doc = new DOMParser().parseFromString(text, 'application/xml');
  if (doc.querySelector('parsererror')) throw new Error('Plik nie jest poprawnym GPX.');
  const name = doc.querySelector('trk > name, rte > name, metadata > name')?.textContent?.trim() || 'Ślad GPX';
  let nodes = Array.from(doc.getElementsByTagName('trkpt'));
  if (!nodes.length) nodes = Array.from(doc.getElementsByTagName('rtept'));
  const points = nodes.map((n) => {
    const e = n.getElementsByTagName('ele')[0]?.textContent;
    return {
      lat: Number(n.getAttribute('lat')),
      lon: Number(n.getAttribute('lon')),
      e: e != null && e !== '' ? Number(e) : undefined,
    };
  }).filter((p) => Number.isFinite(p.lat) && Number.isFinite(p.lon));
  if (points.length < 2) throw new Error('GPX nie zawiera śladu (trkpt/rtept).');
  return { name, points };
}

export function download(filename: string, content: string | Blob, type = 'application/gpx+xml') {
  const blob = typeof content === 'string' ? new Blob([content], { type }) : content;
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}
