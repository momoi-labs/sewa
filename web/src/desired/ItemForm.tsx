import { Button, Form, FormField, RadioGroup, RadioGroupItem, Spinner } from '@momoi-labs/kiso-react';
import { type FormEvent, useState } from 'react';

import type { Item, ItemKind } from '../api';
import { ErrorAlert, errorCode } from '../components/ErrorAlert';

const LIMIT = 200;

/** Why a name or version is refused, as the controller checks it. */
function problem(value: string, what: string): string | undefined {
  if (!value.trim()) return `Enter a ${what}.`;
  if (value.trim().length > LIMIT) return `Use at most ${LIMIT} characters.`;
  return undefined;
}

type Props = {
  /** Sends the item. Rejects with an ApiFailure. */
  onAdd: (item: Item) => Promise<unknown>;
  submitLabel: string;
};

/** Adds an item, or sets its version: a package or a mise tool. */
export function ItemForm({ onAdd, submitLabel }: Props) {
  const [kind, setKind] = useState<ItemKind>('package');
  const [name, setName] = useState('');
  const [version, setVersion] = useState('latest');
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const nameError = checked ? problem(name, 'name') : undefined;
  const versionError = checked
    ? (problem(version, 'version') ??
      (kind === 'tool' && version.trim() === 'absent' ? 'Only a package can be absent.' : undefined))
    : undefined;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setChecked(true);
    if (busy || problem(name, 'name') || problem(version, 'version')) return;
    if (kind === 'tool' && version.trim() === 'absent') return;
    setBusy(true);
    setError(null);
    try {
      await onAdd({ kind, name: name.trim(), version: version.trim() });
      setName('');
      setVersion('latest');
      setChecked(false);
    } catch (failure) {
      setError(failure);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={(event) => void submit(event)} noValidate className="stack">
      {error !== null && errorCode(error) !== 'invalid_item' && <ErrorAlert error={error} />}
      <RadioGroup label="Kind" variant="segmented" value={kind} onValueChange={(value) => setKind(value as ItemKind)}>
        <RadioGroupItem value="package" label="Package" />
        <RadioGroupItem value="tool" label="mise tool" />
      </RadioGroup>
      <div className="row-wrap">
        <FormField
          name="name"
          label="Name"
          placeholder={kind === 'package' ? 'jq' : 'node'}
          hint={kind === 'package' ? 'A package, such as jq or pacman:jq.' : 'A mise tool, such as node.'}
          autoComplete="off"
          className="mono"
          value={name}
          onChange={(event) => setName(event.target.value)}
          error={nameError ?? (errorCode(error) === 'invalid_item' ? 'Check the name and the version.' : undefined)}
        />
        <FormField
          name="version"
          label="Version"
          hint={kind === 'package' ? 'latest, a version, or absent.' : 'latest or a version.'}
          autoComplete="off"
          className="mono"
          value={version}
          onChange={(event) => setVersion(event.target.value)}
          error={versionError}
        />
      </div>
      <div>
        <Button type="submit" aria-busy={busy}>
          {busy && <Spinner size="sm" />}
          {submitLabel}
        </Button>
      </div>
    </Form>
  );
}
