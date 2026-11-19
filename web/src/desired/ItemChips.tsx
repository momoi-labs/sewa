import {
  Chip,
  ChipInput,
  ChipInputBox,
  ChipInputEmpty,
  ChipInputField,
  ChipInputList,
  ChipInputOption,
  ChipName,
  ChipOption,
  ChipOptionAdd,
  ChipRemove,
  ChipValue,
  Label,
  ValidationMessage,
} from '@momoi-labs/kiso-react';
import { type ReactNode, useId, useState } from 'react';

import type { Item, ItemKind } from '../api';
import { isWord, itemKey, itemProblem, parseOs } from './items';

const MAX_SUGGESTIONS = 8;

type Props = {
  value: Item[];
  onChange: (value: Item[]) => void;
  suggestions?: Record<ItemKind, string[]>;
  problem?: (item: Item) => string | null;
  withAbsent?: boolean;
  withOs?: boolean;
};

export function ItemChips({ value, onChange, suggestions, problem = itemProblem, withAbsent, withOs }: Props) {
  const shared = { value, onChange, problem, withOs };
  const absent = withAbsent ? ' Set a version to absent to leave a role item out.' : '';
  const os = withOs ? ' Add the option os=linux or os=macos to install an item on one OS only.' : '';
  return (
    <div className="stack">
      <ItemBox
        {...shared}
        label="Packages"
        kind="package"
        suggestions={suggestions?.package}
        placeholder="jq, brew:jq, pacman:htop…"
        help={
          <>
            Without a prefix a package goes to the host's package manager: pacman on Linux, Homebrew on macOS. Prefix it
            with <code>brew:</code>, <code>brew-cask:</code>, <code>pacman:</code> or <code>aur:</code> to pick one.
            {absent}
            {os}
          </>
        }
      />
      <ItemBox
        {...shared}
        label="Mise tools"
        kind="tool"
        suggestions={suggestions?.tool}
        placeholder="node, python, npm:prettier…"
        help={
          <>
            A tool name such as <code>node</code>, or a backend such as <code>npm:prettier</code>,{' '}
            <code>pipx:ruff</code>, <code>cargo:</code> or <code>go:</code>. The version is <code>latest</code> or a
            number, such as 22.
            {absent}
            {os}
          </>
        }
      />
    </div>
  );
}

function ItemBox({
  value,
  onChange,
  problem,
  withOs,
  label,
  kind,
  suggestions = [],
  placeholder,
  help,
}: Omit<Props, 'suggestions' | 'withAbsent'> & {
  problem: (item: Item) => string | null;
  label: string;
  kind: ItemKind;
  suggestions?: string[];
  placeholder: string;
  help: ReactNode;
}) {
  const id = useId();
  const [query, setQuery] = useState('');
  const [focused, setFocused] = useState(false);
  const typed = query.trim();
  const mine = value.filter((item) => item.kind === kind);
  const taken = mine.some((item) => item.name === typed);
  const valid = isWord(typed);
  const matches = suggestions
    .filter((name) => name.toLowerCase().includes(typed.toLowerCase()) && !mine.some((item) => item.name === name))
    .slice(0, MAX_SUGGESTIONS);
  const invalid = mine.filter((item) => problem(item) !== null);

  function add(name: string) {
    if (!isWord(name) || mine.some((item) => item.name === name)) return;
    onChange([...value, { kind, name, version: 'latest', os: null }]);
    setQuery('');
  }

  function update(target: Item, change: Partial<Item>) {
    onChange(value.map((item) => (itemKey(item) === itemKey(target) ? { ...item, ...change } : item)));
  }

  function remove(target: Item) {
    onChange(value.filter((item) => itemKey(item) !== itemKey(target)));
  }

  return (
    <div className="stack-sm">
      <Label htmlFor={id}>{label}</Label>
      <ChipInput>
        <ChipInputBox
          onFocus={() => setFocused(true)}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocused(false);
          }}
        >
          {mine.map((item) => {
            const absent = item.version === 'absent';
            return (
              <Chip key={item.name} invalid={problem(item) !== null}>
                <ChipName style={absent ? { textDecoration: 'line-through' } : undefined}>{item.name}</ChipName>
                <ChipValue
                  value={item.version}
                  display={absent ? <span className="muted">left out</span> : undefined}
                  editLabel={`Edit ${item.name} version, currently ${item.version}`}
                  confirmLabel={`Confirm ${item.name} version`}
                  onCommit={(version) => update(item, { version: version.trim() || 'latest' })}
                />
                {withOs &&
                  (item.os ? (
                    <ChipOption
                      name="os"
                      value={item.os}
                      editLabel={`Edit ${item.name} OS, currently ${item.os}`}
                      confirmLabel={`Confirm ${item.name} OS`}
                      onCommit={(text) => {
                        const os = text.trim() === '' ? null : parseOs(text);
                        if (os !== undefined) update(item, { os });
                      }}
                    />
                  ) : (
                    <ChipOptionAdd
                      label={`Add ${item.name} options`}
                      placeholder="os=linux"
                      confirmLabel={`Confirm ${item.name} options`}
                      onCommit={(text) => {
                        const os = parseOs(text);
                        if (os !== undefined) update(item, { os });
                      }}
                    />
                  ))}
                <ChipRemove aria-label={`Remove ${item.name}`} onClick={() => remove(item)} />
              </Chip>
            );
          })}
          <ChipInputField
            id={id}
            placeholder={mine.length ? 'Add another…' : placeholder}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onRemoveLast={() => {
              const last = mine.at(-1);
              if (last) remove(last);
            }}
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
            {!taken && valid && !matches.includes(typed) && (
              <ChipInputOption onSelect={() => add(typed)}>
                <span className="mono">{typed}</span>
                <span className="muted">as typed</span>
              </ChipInputOption>
            )}
            {taken && <ChipInputEmpty>The list has {typed}.</ChipInputEmpty>}
            {!valid && !taken && <ChipInputEmpty>A name has no spaces.</ChipInputEmpty>}
          </ChipInputList>
        )}
      </ChipInput>
      <small className="field-hint">{help}</small>
      {invalid.length > 0 && (
        <ValidationMessage>{invalid.map((item) => `${item.name} needs ${problem(item)}.`).join(' ')}</ValidationMessage>
      )}
    </div>
  );
}
