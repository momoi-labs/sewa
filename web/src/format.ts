const dateTime = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'medium' });
const clock = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' });
const relative = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });

/** A moment as a date and time in the browser's zone. */
export function formatDateTime(at: string): string {
  return dateTime.format(new Date(at));
}

/** Says when a late report arrived: the agent kept it while it could not reach the controller. */
export function formatReceived(at: string): string {
  return `Received ${formatDateTime(at)}, kept by the agent while offline.`;
}

/** The time of day of a moment, such as "14:05". */
export function formatClock(at: string): string {
  return clock.format(new Date(at));
}

/** How long ago a moment was, such as "12 seconds ago", from `now`. */
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
