import {
  Button,
  Card,
  Form,
  FormActions,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Spinner,
} from '@momoi-labs/kiso-react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { type FormEvent, useState } from 'react';

import { createRole, type Item, rolesQuery } from '../api';
import { ErrorAlert, errorCode } from '../components/ErrorAlert';
import { itemProblem } from './items';
import { nameProblem, RoleSteps } from './RolePage';

export function NewRolePage() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState('');
  const [items, setItems] = useState<Item[]>([]);
  const [checked, setChecked] = useState(false);
  const mutation = useMutation({
    mutationFn: () => createRole(name.trim(), items),
    onSuccess: async (role) => {
      await queryClient.invalidateQueries({ queryKey: rolesQuery.queryKey });
      await navigate({ to: '/roles/$roleId', params: { roleId: role.id } });
    },
  });
  const nameError = checked || name ? nameProblem(name, mutation.error) : undefined;
  const ready = !nameProblem(name, mutation.error) && items.every((item) => itemProblem(item) === null);

  function submit(event: FormEvent) {
    event.preventDefault();
    setChecked(true);
    if (ready && !mutation.isPending) mutation.mutate();
  }

  return (
    <>
      <PageHeader>
        <PageHeaderTitle>New role</PageHeaderTitle>
        <PageHeaderDescription>
          A role is a set of packages and mise tools that you give to hosts.
        </PageHeaderDescription>
      </PageHeader>
      <Card className="form-page role-form" data-fill="false">
        <Form onSubmit={submit} noValidate>
          <div className="form-body">
            {mutation.isError && errorCode(mutation.error) !== 'name_taken' && (
              <ErrorAlert error={mutation.error} thing="role" />
            )}
            <RoleSteps
              self={null}
              name={name}
              onName={(next) => {
                setName(next);
                if (errorCode(mutation.error) === 'name_taken') mutation.reset();
              }}
              nameError={nameError}
              items={items}
              onItems={setItems}
            />
          </div>
          <FormActions sticky>
            <Button
              type="button"
              size="sm"
              disabled={mutation.isPending}
              onClick={() => void navigate({ to: '/roles' })}
            >
              Cancel
            </Button>
            <Button type="submit" size="sm" variant="primary" disabled={!ready} aria-busy={mutation.isPending}>
              {mutation.isPending && <Spinner size="sm" />}
              Create role
            </Button>
          </FormActions>
        </Form>
      </Card>
    </>
  );
}
