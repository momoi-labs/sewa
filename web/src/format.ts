const dateTime = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'medium' });
const clock = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' });
const relative = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });

export function formatDateTime(at: string): string {
  return dateTime.format(new Date(at));
}

export function formatReceived(at: string): string {
  return `Received ${formatDateTime(at)}, kept by the agent while offline.`;
}

export function formatClock(at: string): string {
  return clock.format(new Date(at));
}

export function formatAgo(at: string, now: number): string {
  const seconds = Math.round((new Date(at).getTime() - now) / 1000);
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ['day', 86400],
    ['hour', 3600],
    ['minute', 60],
  ];
  for (const [unit, size] of steps) {
    if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
  }
  return relative.format(Math.min(seconds, 0), 'second');
}

export const osNames: Record<string, string> = { linux: 'Linux', macos: 'macOS' };
