import {
  Badge,
  Button,
  Card,
  Form,
  FormActions,
  FormField,
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
import { diffItems, diffSize, itemProblem } from './items';
import { ItemTable } from './ItemTable';
import { SaveRoleDialog } from './SaveRoleDialog';

const route = getRouteApi('/roles/$roleId');

const NAME = /^[A-Za-z0-9._-]{1,63}$/;

export function RolePage() {
  const { roleId } = route.useParams();
  const role = useQuery(roleQuery(roleId));

  return (
    <>
      {role.isError && !role.data ? (
        <ErrorAlert error={role.error} thing="role" onRetry={() => void role.refetch()} />
      ) : (
        <>
          <PageHeader>
            <div className="row-wrap">
              <PageHeaderTitle>{role.data?.name ?? <Skeleton />}</PageHeaderTitle>
              {role.data?.builtin && <Badge variant="neutral">Built in</Badge>}
            </div>
            <PageHeaderDescription>
              {role.data?.builtin
                ? 'What sewa needs on every host.'
                : 'Packages and mise tools for the hosts with this role. Give it to a host on the host page.'}
            </PageHeaderDescription>
          </PageHeader>
          <Card className="form-page role-form">
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

function namesOf(roles: Role[] | undefined, self: string, kind: Item['kind']): string[] {
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

function RoleForm({ role }: { role: Role }) {
  const roles = useQuery(rolesQuery);
  const [saved, setSaved] = useState(role);
  const [name, setName] = useState(role.name);
  const [items, setItems] = useState(role.items);
  const [confirming, setConfirming] = useState(false);
  const [failure, setFailure] = useState<unknown>(null);

  const diff = diffItems(saved.items, items);
  const renamed = name.trim() !== saved.name;
  const dirty = renamed || diffSize(diff) > 0;
  const nameProblem = !name.trim()
    ? 'Enter a name.'
    : !NAME.test(name.trim())
      ? 'Use up to 63 letters, digits, dots, dashes or underscores.'
      : errorCode(failure) === 'name_taken'
        ? 'A role has this name. Choose another.'
        : undefined;
  const ready = !nameProblem && items.every((item) => itemProblem(item) === null);

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
        <p className="t-caps">Configuration</p>
        {failure !== null && errorCode(failure) !== 'name_taken' && <ErrorAlert error={failure} thing="role" />}
        <ol className="form-steps">
          <li data-done={!nameProblem || undefined}>
            <p className="t-caps">Role</p>
            <FormField
              name="name"
              label="Name"
              autoComplete="off"
              value={name}
              hint="Letters, digits, dots, dashes or underscores."
              onChange={(event) => {
                setName(event.target.value);
                if (errorCode(failure) === 'name_taken') setFailure(null);
              }}
              error={nameProblem}
            />
          </li>
          <li data-done={items.length > 0 || undefined}>
            <p className="t-caps">Items</p>
            <ItemTable
              value={items}
              onChange={setItems}
              suggestions={{
                package: namesOf(roles.data, role.id, 'package'),
                tool: namesOf(roles.data, role.id, 'tool'),
              }}
            />
          </li>
        </ol>
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
