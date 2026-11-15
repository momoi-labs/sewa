import {
  Button,
  ChipInput,
  ChipInputBox,
  ChipInputEmpty,
  ChipInputField,
  ChipInputList,
  ChipInputOption,
  Disclosure,
  Input,
  Search,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
  ValidationMessage,
} from '@momoi-labs/kiso-react';
import { X } from 'lucide-react';
import { useState } from 'react';

import type { Item, ItemKind, Os } from '../api';
import { type GroupKey, groupOf, groups, isWord, itemKey, itemProblem, splitKey } from './items';

const MAX_SUGGESTIONS = 8;

/** The items in their groups, sorted by name, with empty groups left out. */
export function groupItems(items: Item[], query = '') {
  const typed = query.trim().toLowerCase();
  return groups
    .map((group) => {
      const all = items.filter((item) => groupOf(item) === group.key);
      const shown = all
        .filter((item) => !typed || item.name.toLowerCase().includes(typed))
        .sort((a, b) => a.name.localeCompare(b.name));
      return { ...group, total: all.length, items: shown };
    })
    .filter((group) => group.total > 0);
}

const osNames: Record<Os | 'all', string> = { all: 'Linux and macOS', linux: 'Linux only', macos: 'macOS only' };

type Props = {
  /** Every item of the role, packages and mise tools. */
  value: Item[];
  onChange?: (value: Item[]) => void;
  /** Names to suggest per kind, such as the items of other roles. */
  suggestions?: Record<ItemKind, string[]>;
  /** Shows the items with no edit controls. */
  readOnly?: boolean;
};

/** A role's items as tables, one per source, in sections that fold. A
 *  search filters the rows; an add field takes a package or a mise tool
 *  with the prefix syntax. Version and OS change in the row. */
export function ItemTable({ value, onChange, suggestions, readOnly }: Props) {
  const [query, setQuery] = useState('');
  const [closed, setClosed] = useState<Set<GroupKey>>(new Set());
  const searching = query.trim() !== '';
  const shown = groupItems(value, query);
  const matches = shown.reduce((sum, group) => sum + group.items.length, 0);
  const invalid = value.filter((item) => itemProblem(item) !== null);

  function update(target: Item, change: Partial<Item>) {
    onChange?.(value.map((item) => (itemKey(item) === itemKey(target) ? { ...item, ...change } : item)));
  }

  function add(item: Item) {
    if (value.some((other) => itemKey(other) === itemKey(item))) return;
    onChange?.([...value, item]);
    const group = groupOf(item);
    setClosed((before) => new Set([...before].filter((key) => key !== group)));
  }

  return (
    <div className="stack">
      <div className="item-toolbar">
        <Search
          aria-label="Filter items"
          placeholder="Filter by name"
          containerClassName="item-search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        {!readOnly && <AddItem have={value} suggestions={suggestions} onAdd={add} />}
      </div>
      {!readOnly && (
        <small className="field-hint">
          A package without a prefix goes to the host's package manager: pacman on Linux, Homebrew on macOS. Prefix it
          with <code>brew:</code>, <code>brew-cask:</code>, <code>pacman:</code> or <code>aur:</code> to pick one. A
          version is <code>latest</code>, a version, or <code>absent</code> for a package.
        </small>
      )}
      {invalid.length > 0 && (
        <ValidationMessage>
          {invalid.map((item) => `${item.name} needs ${itemProblem(item)}.`).join(' ')}
        </ValidationMessage>
      )}
      {value.length === 0 ? (
        <p className="muted">No items yet. Add a package or a mise tool above.</p>
      ) : searching && matches === 0 ? (
        <p className="muted" role="status">
          No item matches "{query.trim()}".
        </p>
      ) : (
        shown
          .filter((group) => group.items.length > 0)
          .map((group) => (
            <Disclosure
              key={group.key}
              className="item-group"
              open={searching || !closed.has(group.key)}
              onToggle={(event) => {
                if (searching) return;
                const open = event.currentTarget.open;
                setClosed((before) => {
                  const next = new Set(before);
                  if (open) next.delete(group.key);
                  else next.add(group.key);
                  return next;
                });
              }}
              summary={
                <>
                  {group.label}{' '}
                  <span className="muted">
                    ({searching ? `${group.items.length} of ${group.total}` : group.total})
                  </span>
                </>
              }
            >
              <TableFrame className="item-table" data-shadow-style="none">
                <Table density="compact" aria-label={group.label}>
                  <colgroup>
                    <col />
                    <col className="item-col-manager" />
                    <col className="item-col-version" />
                    <col className="item-col-os" />
                    {!readOnly && <col className="item-col-remove" />}
                  </colgroup>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Name</TableHead>
                      <TableHead>Manager</TableHead>
                      <TableHead>Version</TableHead>
                      <TableHead>OS</TableHead>
                      {!readOnly && (
                        <TableHead aria-label="Remove" />
                      )}
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {group.items.map((item) => (
                      <ItemRow
                        key={itemKey(item)}
                        item={item}
                        manager={group.manager || (splitKey(item.name).scope ?? '')}
                        readOnly={readOnly}
                        onUpdate={(change) => update(item, change)}
                        onRemove={() => onChange?.(value.filter((other) => itemKey(other) !== itemKey(item)))}
                      />
                    ))}
                  </TableBody>
                </Table>
              </TableFrame>
            </Disclosure>
          ))
      )}
    </div>
  );
}

function ItemRow({
  item,
  manager,
  readOnly,
  onUpdate,
  onRemove,
}: {
  item: Item;
  manager: string;
  readOnly?: boolean;
  onUpdate: (change: Partial<Item>) => void;
  onRemove: () => void;
}) {
  const name = item.kind === 'tool' ? item.name : splitKey(item.name).name;
  const problem = itemProblem(item);
  return (
    <TableRow data-invalid={problem !== null || undefined}>
      <TableCell className="item-name mono">{name}</TableCell>
      <TableCell className="item-manager" data-label="Manager">
        {manager}
      </TableCell>
      <TableCell className="item-version" data-label="Version">
        {readOnly ? (
          <span className="mono">{item.version}</span>
        ) : (
          <Input
            controlSize="sm"
            className="mono"
            aria-label={`Version of ${item.name}`}
            aria-invalid={problem !== null || undefined}
            value={item.version}
            onChange={(event) => onUpdate({ version: event.target.value.trim() })}
          />
        )}
      </TableCell>
      <TableCell className="item-os" data-label="OS">
        {readOnly ? (
          osNames[item.os ?? 'all']
        ) : (
          <Select
            value={item.os ?? 'all'}
            onValueChange={(next) => onUpdate({ os: next === 'all' ? null : (next as Os) })}
          >
            <SelectTrigger controlSize="sm" aria-label={`OS of ${item.name}`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(['all', 'linux', 'macos'] as const).map((os) => (
                <SelectItem key={os} value={os}>
                  {osNames[os]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
      </TableCell>
      {!readOnly && (
        <TableCell className="item-remove">
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="btn-icon"
            aria-label={`Remove ${item.name}`}
            onClick={onRemove}
          >
            <X aria-hidden="true" />
          </Button>
        </TableCell>
      )}
    </TableRow>
  );
}

/** The add field: a kind, then a name with the prefix syntax. Enter adds
 *  the typed name; the suggestions are the names other roles use. */
function AddItem({
  have,
  suggestions,
  onAdd,
}: {
  have: Item[];
  suggestions?: Record<ItemKind, string[]>;
  onAdd: (item: Item) => void;
}) {
  const [kind, setKind] = useState<ItemKind>('package');
  const [query, setQuery] = useState('');
  const [focused, setFocused] = useState(false);
  const typed = query.trim();
  const taken = new Set(have.filter((item) => item.kind === kind).map((item) => item.name));
  const matches = typed
    ? (suggestions?.[kind] ?? [])
        .filter((name) => !taken.has(name) && name.toLowerCase().includes(typed.toLowerCase()))
        .slice(0, MAX_SUGGESTIONS)
    : [];
  const label = kind === 'package' ? 'Add a package' : 'Add a mise tool';

  function add(name: string) {
    setQuery('');
    if (!isWord(name) || taken.has(name)) return;
    onAdd({ kind, name, version: 'latest', os: null });
  }

  return (
    <div className="item-add">
      <Select value={kind} onValueChange={(next) => setKind(next as ItemKind)}>
        <SelectTrigger controlSize="md" aria-label="Kind to add">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="package">Package</SelectItem>
          <SelectItem value="tool">mise tool</SelectItem>
        </SelectContent>
      </Select>
      <ChipInput className="grow">
        <ChipInputBox
          onFocus={() => setFocused(true)}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocused(false);
          }}
        >
          <ChipInputField
            aria-label={label}
            placeholder={kind === 'package' ? 'jq, brew:jq, pacman:htop…' : 'node, python, npm:prettier…'}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.defaultPrevented) return;
              if (event.key === 'Enter') {
                event.preventDefault();
                add(typed);
              }
            }}
          />
        </ChipInputBox>
        {focused && typed && (
          <ChipInputList aria-label={`${label}: suggestions`}>
            {matches.map((name) => (
              <ChipInputOption key={name} onSelect={() => add(name)}>
                <span className="mono">{name}</span>
              </ChipInputOption>
            ))}
            {!taken.has(typed) && !matches.includes(typed) && isWord(typed) && (
              <ChipInputOption onSelect={() => add(typed)}>
                <span className="mono">{typed}</span>
                <span className="muted">as typed</span>
              </ChipInputOption>
            )}
            {taken.has(typed) && <ChipInputEmpty>The role has {typed}.</ChipInputEmpty>}
            {matches.length === 0 && !isWord(typed) && <ChipInputEmpty>A name has no spaces.</ChipInputEmpty>}
          </ChipInputList>
        )}
      </ChipInput>
    </div>
  );
}
