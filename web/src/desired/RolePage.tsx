import {
  Badge,
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
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
import { getRouteApi, Link } from '@tanstack/react-router';
import { type FormEvent, type ReactNode, useState } from 'react';

import { type Item, type Role, roleQuery, rolesQuery } from '../api';
import { ErrorAlert, errorCode } from '../components/ErrorAlert';
import { ItemChips } from './ItemChips';
import { diffItems, diffSize, itemProblem } from './items';
import { SaveRoleDialog } from './SaveRoleDialog';

const route = getRouteApi('/roles/$roleId');

const NAME = /^[A-Za-z0-9._-]{1,63}$/;

/** One role, edited in numbered steps: its name, its packages, and its mise
 *  tools. Save shows what changes and asks to confirm. Hosts get roles on
 *  the host page. */
export function RolePage() {
  const { roleId } = route.useParams();
  const role = useQuery(roleQuery(roleId));

  return (
    <>
      <Breadcrumb aria-label="Breadcrumb">
        <BreadcrumbList>
          <BreadcrumbItem>
            <BreadcrumbLink asChild>
              <Link to="/roles">Roles</Link>
            </BreadcrumbLink>
          </BreadcrumbItem>
          <BreadcrumbSeparator />
          <BreadcrumbItem>
            <BreadcrumbPage>{role.data?.name ?? '…'}</BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </Breadcrumb>
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
                ? 'What sewa needs on every host. It holds packages only.'
                : 'Packages and mise tools for the hosts with this role. Give it to a host on the host page.'}
            </PageHeaderDescription>
          </PageHeader>
          <Card className="form-page">
            {role.data ? <RoleForm key={role.data.id} role={role.data} /> : <Skeleton />}
          </Card>
        </>
      )}
    </>
  );
}

/** The names other roles use for this kind, to suggest. */
function namesOf(roles: Role[] | undefined, self: string, kind: Item['kind']): string[] {
  const names = new Set<string>();
  for (const role of roles ?? []) {
    if (role.id === self) continue;
    for (const item of role.items) if (item.kind === kind) names.add(item.name);
  }
  return [...names].sort();
}

function RoleForm({ role }: { role: Role }) {
  const roles = useQuery(rolesQuery);
  const [saved, setSaved] = useState(role);
  const [name, setName] = useState(role.name);
  const [packages, setPackages] = useState(role.items.filter((item) => item.kind === 'package'));
  const [tools, setTools] = useState(role.items.filter((item) => item.kind === 'tool'));
  const [confirming, setConfirming] = useState(false);
  const [failure, setFailure] = useState<unknown>(null);

  const items = [...packages, ...tools];
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
    setPackages(saved.items.filter((item) => item.kind === 'package'));
    setTools(saved.items.filter((item) => item.kind === 'tool'));
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
              disabled={role.builtin}
              hint={role.builtin ? 'The built-in role keeps its name.' : 'Letters, digits, dots, dashes or underscores.'}
              onChange={(event) => {
                setName(event.target.value);
                if (errorCode(failure) === 'name_taken') setFailure(null);
              }}
              error={nameProblem}
            />
          </li>
          <li data-done={packages.length > 0 || undefined}>
            <p className="t-caps">Packages</p>
            <ItemChips
              id="role-packages"
              kind="package"
              label="Packages"
              placeholder="jq, brew:jq, pacman:htop…"
              hint={
                <>
                  A package without a prefix goes to the host's package manager: pacman on Linux, Homebrew on macOS.
                  Prefix it with <code>brew:</code>, <code>brew-cask:</code>, <code>pacman:</code> or <code>aur:</code>{' '}
                  to pick one. Press a version to change it: <code>latest</code>, a version, or <code>absent</code>.
                  Press <code>+</code> and type <code>os=linux</code> or <code>os=macos</code> for one OS only.
                </>
              }
              value={packages}
              onChange={setPackages}
              suggestions={namesOf(roles.data, role.id, 'package')}
            />
          </li>
          {!role.builtin && (
            <li data-done={tools.length > 0 || undefined}>
              <p className="t-caps">mise tools</p>
              <ItemChips
                id="role-tools"
                kind="tool"
                label="mise tools"
                placeholder="node, python, npm:prettier…"
                hint={
                  <>
                    Type a mise tool, or pick one that another role uses. Press a version to change it. Press{' '}
                    <code>+</code> and type <code>os=linux</code> or <code>os=macos</code> for one OS only.
                  </>
                }
                value={tools}
                onChange={setTools}
                suggestions={namesOf(roles.data, role.id, 'tool')}
              />
            </li>
          )}
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
          setPackages(next.items.filter((item) => item.kind === 'package'));
          setTools(next.items.filter((item) => item.kind === 'tool'));
          setFailure(null);
        }}
        onFailed={setFailure}
      />
    </Form>
  );
}
