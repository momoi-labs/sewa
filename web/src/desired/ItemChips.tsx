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
  ChipScope,
  ChipValue,
  Label,
  ValidationMessage,
} from '@momoi-labs/kiso-react';
import { type ReactNode, useState } from 'react';

import type { Item, ItemKind } from '../api';
import { isWord, itemKey, itemProblem, parseOs, splitKey } from './items';

const MAX_SUGGESTIONS = 8;

type Props = {
  id: string;
  kind: ItemKind;
  label: string;
  hint: ReactNode;
  placeholder: string;
  /** The items of this kind, in order. */
  value: Item[];
  onChange: (value: Item[]) => void;
  /** Names to suggest, such as the items of other roles. */
  suggestions: string[];
  disabled?: boolean;
};

/** Items as chips: the manager prefix, the name, the version, and an
 *  optional `os` option. Typing a name adds it at `latest`. */
export function ItemChips({ id, kind, label, hint, placeholder, value, onChange, suggestions, disabled }: Props) {
  const [query, setQuery] = useState('');
  const [focused, setFocused] = useState(false);
  const typed = query.trim();
  const have = new Set(value.map((item) => item.name));
  const matches = typed
    ? suggestions
        .filter((name) => !have.has(name) && name.toLowerCase().includes(typed.toLowerCase()))
        .slice(0, MAX_SUGGESTIONS)
    : [];
  const invalid = value.filter((item) => itemProblem(item) !== null);

  function add(name: string) {
    setQuery('');
    if (!isWord(name) || have.has(name)) return;
    onChange([...value, { kind, name, version: 'latest', os: null }]);
  }

  function update(name: string, change: Partial<Item>) {
    onChange(value.map((item) => (item.name === name ? { ...item, ...change } : item)));
  }

  function commitOs(item: Item, text: string) {
    const os = parseOs(text);
    if (os !== undefined) update(item.name, { os });
  }

  return (
    <div className="field">
      <Label htmlFor={id}>{label}</Label>
      <ChipInput>
        <ChipInputBox
          disabled={disabled}
          onFocus={() => setFocused(true)}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocused(false);
          }}
        >
          {value.map((item) => {
            const { scope, name } = splitKey(item.name);
            return (
              <Chip key={itemKey(item)} invalid={itemProblem(item) !== null}>
                {scope && <ChipScope>{scope}</ChipScope>}
                <ChipName>{name}</ChipName>
                <ChipValue
                  value={item.version}
                  editable={!disabled}
                  editLabel={`Edit ${item.name} version, currently ${item.version}`}
                  confirmLabel={`Confirm ${item.name} version`}
                  onCommit={(version) => update(item.name, { version: version.trim() })}
                />
                {item.os ? (
                  <ChipOption
                    name="os"
                    value={item.os}
                    label={item.name}
                    editable={!disabled}
                    onCommit={(text) => commitOs(item, text)}
                  />
                ) : (
                  !disabled && <ChipOptionAdd label={item.name} onCommit={(text) => commitOs(item, text)} />
                )}
                <ChipRemove
                  aria-label={`Remove ${item.name}`}
                  disabled={disabled}
                  onClick={() => onChange(value.filter((other) => other.name !== item.name))}
                />
              </Chip>
            );
          })}
          <ChipInputField
            id={id}
            aria-describedby={`${id}-help`}
            placeholder={placeholder}
            disabled={disabled}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onRemoveLast={() => onChange(value.slice(0, -1))}
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
          <ChipInputList aria-label={`${label} suggestions`}>
            {matches.map((name) => (
              <ChipInputOption key={name} onSelect={() => add(name)}>
                <span className="mono">{name}</span>
              </ChipInputOption>
            ))}
            {!have.has(typed) && !matches.includes(typed) && isWord(typed) && (
              <ChipInputOption onSelect={() => add(typed)}>
                <span className="mono">{typed}</span>
                <span className="muted">as typed</span>
              </ChipInputOption>
            )}
            {matches.length === 0 && !isWord(typed) && <ChipInputEmpty>A name has no spaces.</ChipInputEmpty>}
          </ChipInputList>
        )}
      </ChipInput>
      <small className="field-hint" id={`${id}-help`}>
        {hint}
      </small>
      {invalid.length > 0 && (
        <ValidationMessage>
          {invalid.map((item) => `${item.name} needs ${itemProblem(item)}.`).join(' ')}
        </ValidationMessage>
      )}
    </div>
  );
}
