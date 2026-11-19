import {
  Badge,
  Button,
  Card,
  Form,
  FormActions,
  FormField,
  Lifecycle,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Skeleton,
} from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { getRouteApi } from '@tanstack/react-router';
import { type FormEvent, type ReactNode, useState } from 'react';

import { type Item, type Role, roleQuery, rolesQuery } from '../api';
import { ErrorAlert, errorCode } from '../components/ErrorAlert';
import { plural } from '../sync/sync';
import { ItemChips } from './ItemChips';
import { diffItems, diffSize, itemProblem } from './items';
import { ItemTable } from './ItemTable';
import { SaveRoleDialog } from './SaveRoleDialog';

const route = getRouteApi('/roles/$roleId');

const NAME = /^[A-Za-z0-9._-]{1,63}$/;

export function nameProblem(name: string, failure: unknown): string | undefined {
  if (!name.trim()) return 'Enter a name.';
  if (!NAME.test(name.trim())) return 'Use up to 63 letters, digits, dots, dashes or underscores.';
  if (errorCode(failure) === 'name_taken') return 'A role has this name. Choose another.';
  return undefined;
}

export function RolePage() {
  const { roleId } = route.useParams();
  const role = useQuery(roleQuery(roleId));

  return (
    <>
      {role.isError && !role.data ? (
        <ErrorAlert error={role.error} thing="role" onRetry={() => void role.refetch()} />
      ) : (
        <>
          <div className="between">
            <PageHeader>
              <PageHeaderTitle>{role.data?.name ?? <Skeleton />}</PageHeaderTitle>
              <PageHeaderDescription>
                {role.data?.builtin
                  ? 'What sewa needs on every host.'
                  : 'Packages and mise tools for the hosts with this role. Give it to a host on the host page.'}
              </PageHeaderDescription>
            </PageHeader>
            {role.data && (
              <Lifecycle
                status={
                  <Badge variant="neutral">
                    {role.data.builtin ? 'Built in' : `Given to ${plural(role.data.hosts.length, 'host')}`}
                  </Badge>
                }
              />
            )}
          </div>
          <Card className="detail-tabs role-form" data-fill="false">
            {!role.data ? (
              <Skeleton />
            ) : role.data.builtin ? (
              <BuiltinRole role={role.data} />
            ) : (
              <RoleForm key={role.data.id} role={role.data} />
            )}
          </Card>
        </>
      )}
    </>
  );
}

export function namesOf(roles: Role[] | undefined, self: string | null, kind: Item['kind']): string[] {
  const names = new Set<string>();
  for (const role of roles ?? []) {
    if (role.id === self) continue;
    for (const item of role.items) if (item.kind === kind) names.add(item.name);
  }
  return [...names].sort();
}

function BuiltinRole({ role }: { role: Role }) {
  return (
    <div className="form-body">
      <p>sewa manages this role, so it is read-only. Put your own packages in another role.</p>
      <ItemTable value={role.items} readOnly />
    </div>
  );
}

export function RoleSteps({
  self,
  name,
  onName,
  nameError,
  items,
  onItems,
}: {
  self: string | null;
  name: string;
  onName: (name: string) => void;
  nameError: string | undefined;
  items: Item[];
  onItems: (items: Item[]) => void;
}) {
  const roles = useQuery(rolesQuery);
  return (
    <ol className="form-steps">
      <li data-done={(name.trim() && !nameError) || undefined}>
        <p className="t-caps">Role</p>
        <FormField
          name="name"
          label="Name"
          autoComplete="off"
          placeholder="base"
          maxLength={63}
          value={name}
          hint="Letters, digits, dots, dashes or underscores."
          onChange={(event) => onName(event.target.value)}
          error={nameError}
        />
      </li>
      <li data-done={items.length > 0 || undefined}>
        <p className="t-caps">Items</p>
        <ItemChips
          value={items}
          onChange={onItems}
          withOs
          suggestions={{
            package: namesOf(roles.data, self, 'package'),
            tool: namesOf(roles.data, self, 'tool'),
          }}
        />
      </li>
    </ol>
  );
}

function RoleForm({ role }: { role: Role }) {
  const [saved, setSaved] = useState(role);
  const [name, setName] = useState(role.name);
  const [items, setItems] = useState(role.items);
  const [confirming, setConfirming] = useState(false);
  const [failure, setFailure] = useState<unknown>(null);

  const diff = diffItems(saved.items, items);
  const renamed = name.trim() !== saved.name;
  const dirty = renamed || diffSize(diff) > 0;
  const nameError = nameProblem(name, failure);
  const ready = !nameError && items.every((item) => itemProblem(item) === null);

  function discard() {
    setName(saved.name);
    setItems(saved.items);
    setFailure(null);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (dirty && ready) setConfirming(true);
  }

  let message: ReactNode;
  if (dirty)
    message = (
      <>
        <strong>Unsaved edits.</strong> Saving changes the desired state. No host changes until you sync.
      </>
    );
  else message = 'Saved. Hosts get the role at their next sync.';

  return (
    <Form onSubmit={submit} noValidate>
      <div className="form-body">
        {failure !== null && errorCode(failure) !== 'name_taken' && <ErrorAlert error={failure} thing="role" />}
        <RoleSteps
          self={role.id}
          name={name}
          onName={(next) => {
            setName(next);
            if (errorCode(failure) === 'name_taken') setFailure(null);
          }}
          nameError={nameError}
          items={items}
          onItems={setItems}
        />
      </div>
      <FormActions sticky tone={dirty ? 'warning' : 'neutral'} message={message}>
        {dirty && (
          <Button type="button" size="sm" onClick={discard}>
            Discard
          </Button>
        )}
        <Button type="submit" size="sm" variant="primary" disabled={!dirty || !ready}>
          Save
        </Button>
      </FormActions>
      <SaveRoleDialog
        open={confirming}
        onOpenChange={setConfirming}
        role={saved}
        name={name.trim()}
        items={items}
        diff={diff}
        onSaved={(next) => {
          setSaved(next);
          setName(next.name);
          setItems(next.items);
          setFailure(null);
        }}
        onFailed={setFailure}
      />
    </Form>
  );
}
