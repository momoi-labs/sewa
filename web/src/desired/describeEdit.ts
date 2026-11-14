import type { Edit } from '../api';

const kinds = { package: 'package', tool: 'mise tool' } as const;

/** One sentence for an edit. `name` finds a role's or a host's name by id. */
export function describeEdit(edit: Edit, name: (id: string) => string): string {
  switch (edit.op) {
    case 'role_item_set':
      return `Role ${name(edit.role)}: set ${kinds[edit.kind]} ${edit.name} to ${edit.version}`;
    case 'role_item_remove':
      return `Role ${name(edit.role)}: remove ${kinds[edit.kind]} ${edit.name}`;
    case 'host_role_add':
      return `Host ${name(edit.host)}: give role ${name(edit.role)}`;
    case 'host_role_remove':
      return `Host ${name(edit.host)}: take away role ${name(edit.role)}`;
    case 'host_item_set':
      return `Host ${name(edit.host)}: set ${kinds[edit.kind]} ${edit.name} to ${edit.version}`;
    case 'host_item_remove':
      return `Host ${name(edit.host)}: leave out ${kinds[edit.kind]} ${edit.name}`;
    case 'host_item_reset':
      return `Host ${name(edit.host)}: follow the roles for ${kinds[edit.kind]} ${edit.name}`;
    default:
      return 'Unknown edit';
  }
}
