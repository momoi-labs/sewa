import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@momoi-labs/kiso-react';

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
