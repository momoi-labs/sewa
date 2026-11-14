import { Badge } from '@momoi-labs/kiso-react';
import { Minus, Pencil, Plus } from 'lucide-react';

import type { Difference } from './compare';

/** What the draft does to a value: a label and an icon, with the color as
 *  reinforcement only (Kiso value comparison). */
export function DifferenceBadge({ difference }: { difference: Difference }) {
  switch (difference) {
    case 'added':
      return (
        <Badge variant="success">
          <Plus className="icon-sm" aria-hidden="true" />
          Added in draft
        </Badge>
      );
    case 'removed':
      return (
        <Badge variant="danger">
          <Minus className="icon-sm" aria-hidden="true" />
          Removed in draft
        </Badge>
      );
    case 'changed':
      return (
        <Badge variant="warning">
          <Pencil className="icon-sm" aria-hidden="true" />
          Changed in draft
        </Badge>
      );
    default:
      return null;
  }
}
