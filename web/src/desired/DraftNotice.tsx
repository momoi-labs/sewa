import { Alert, AlertContent, AlertDescription, AlertTitle, Button } from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';

import { changesQuery } from '../api';

/** Says that edits wait in the draft change, with a link to its plans. */
export function DraftNotice() {
  const changes = useQuery(changesQuery);
  const draft = changes.data?.find((change) => change.status === 'draft');
  return (
    <Alert variant="info">
      <AlertContent>
        <AlertTitle>{draft ? 'Your edits wait in a draft change' : 'Edits go into a draft change'}</AlertTitle>
        <AlertDescription>
          {draft
            ? `The draft has ${draft.edit_count === 1 ? '1 edit' : `${draft.edit_count} edits`}. Each host plans it; nothing is applied.`
            : 'No host changes until a change is approved. Each host plans the draft first.'}
        </AlertDescription>
      </AlertContent>
      {draft && (
        <Button size="sm" asChild>
          <Link to="/changes/$changeId" params={{ changeId: draft.id }}>
            Open the draft
          </Link>
        </Button>
      )}
    </Alert>
  );
}
