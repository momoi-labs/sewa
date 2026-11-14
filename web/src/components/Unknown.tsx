import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@momoi-labs/kiso-react';

/** The glyph for a value nothing has reported yet: a muted dash whose reason
 *  is a Tooltip and its accessible name (Kiso data interfaces). */
export function Unknown({ reason }: { reason: string }) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="muted" tabIndex={0} role="img" aria-label={reason}>
            —
          </span>
        </TooltipTrigger>
        <TooltipContent>{reason}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
