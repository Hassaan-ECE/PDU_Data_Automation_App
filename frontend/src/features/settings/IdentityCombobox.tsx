import { useId, useState, type KeyboardEvent } from "react";

import { cn } from "@/shared/lib/utils";

import type { StationCatalogEntry, StationRole } from "./settingsTypes";

type IdentityOption = {
  key: string;
  label: string;
  run: () => void;
};

export type IdentityComboboxProps = {
  label: string;
  entries: StationCatalogEntry[];
  value: string;
  disabled?: boolean;
  allowCreate?: boolean;
  placeholder?: string;
  onValueChange: (value: string) => void;
  onSelect: (entry: StationCatalogEntry) => void;
  onCreate?: (name: string, role: StationRole) => void;
};

function identityRoleLabel(role: StationRole) {
  return role === "admin" ? "Admin Identity" : "Floor Station";
}

export function IdentityCombobox({
  label,
  entries,
  value,
  disabled = false,
  allowCreate = false,
  placeholder,
  onValueChange,
  onSelect,
  onCreate,
}: IdentityComboboxProps) {
  const generatedId = useId().replaceAll(":", "");
  const listId = `identity-${generatedId}-listbox`;
  const [open, setOpen] = useState(false);
  const [searchActive, setSearchActive] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const trimmedValue = value.trim();
  const query = searchActive ? trimmedValue.toLocaleLowerCase() : "";
  const matches = entries.filter((entry) =>
    entry.name.toLocaleLowerCase().includes(query),
  );
  const exact = entries.some(
    (entry) => entry.name.toLocaleLowerCase() === trimmedValue.toLocaleLowerCase(),
  );
  const options: IdentityOption[] = matches.map((entry) => ({
    key: entry.id,
    label: `${entry.name} — ${identityRoleLabel(entry.role)}`,
    run: () => onSelect(entry),
  }));

  if (allowCreate && trimmedValue && !exact && onCreate) {
    options.push(
      {
        key: "create-floor",
        label: `Add ${trimmedValue} as Floor Station`,
        run: () => onCreate(trimmedValue, "floor"),
      },
      {
        key: "create-admin",
        label: `Add ${trimmedValue} as Admin Identity`,
        run: () => onCreate(trimmedValue, "admin"),
      },
    );
  }

  const boundedActiveIndex =
    options.length === 0 ? -1 : Math.min(Math.max(activeIndex, 0), options.length - 1);

  function choose(index: number) {
    const option = options[index];
    if (!option) return;
    option.run();
    setOpen(false);
    setSearchActive(false);
    setActiveIndex(-1);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setOpen(true);
      setActiveIndex((index) => Math.min(index + 1, options.length - 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setOpen(true);
      setActiveIndex((index) =>
        index < 0 ? Math.max(options.length - 1, 0) : Math.max(index - 1, 0),
      );
      return;
    }
    if (event.key === "Enter" && open && boundedActiveIndex >= 0) {
      event.preventDefault();
      choose(boundedActiveIndex);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
      setSearchActive(false);
      setActiveIndex(-1);
    }
  }

  return (
    <label className="relative block text-[8pt] font-semibold text-[#d8d2c8]">
      {label}
      <input
        role="combobox"
        aria-label={label}
        aria-autocomplete="list"
        aria-expanded={open}
        aria-controls={listId}
        aria-activedescendant={
          open && boundedActiveIndex >= 0
            ? `${listId}-${boundedActiveIndex}`
            : undefined
        }
        autoComplete="off"
        value={value}
        disabled={disabled}
        maxLength={64}
        placeholder={placeholder}
        onFocus={() => {
          setOpen(true);
          setSearchActive(false);
          setActiveIndex(-1);
        }}
        onChange={(event) => {
          onValueChange(event.target.value);
          setOpen(true);
          setSearchActive(true);
          setActiveIndex(-1);
        }}
        onKeyDown={handleKeyDown}
        className="mt-1 h-9 w-full rounded-md border border-[#454542] bg-[#1f1f1e] px-2.5 text-[8.5pt] text-white placeholder:text-[#777772] outline-none transition focus:border-[#1f74ae] focus:ring-2 focus:ring-cyan-200/25 disabled:cursor-not-allowed disabled:opacity-60"
      />
      {open ? (
        <div
          id={listId}
          role="listbox"
          aria-label={`${label} options`}
          className="absolute z-30 mt-1 max-h-52 w-full overflow-y-auto rounded-md border border-[#555550] bg-[#20201f] p-1 shadow-xl"
        >
          {options.length > 0 ? (
            options.map((option, index) => (
              <button
                key={option.key}
                id={`${listId}-${index}`}
                type="button"
                role="option"
                aria-selected={index === boundedActiveIndex}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => choose(index)}
                className={cn(
                  "block w-full rounded px-2 py-2 text-left text-[8pt] font-medium text-[#d8d2c8]",
                  index === boundedActiveIndex
                    ? "bg-[#1f74ae] text-white"
                    : "hover:bg-[#30302f]",
                )}
              >
                {option.label}
              </button>
            ))
          ) : (
            <div className="px-2 py-2 text-[7.5pt] font-normal text-[#9a958c]">
              No matching identities
            </div>
          )}
        </div>
      ) : null}
    </label>
  );
}
