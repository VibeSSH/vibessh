import * as RadixSelect from "@radix-ui/react-select";
import { Icon } from "./Icon";
import "./Select.css";

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface SelectGroup {
  label: string;
  options: SelectOption[];
}

export type SelectItem = SelectOption | SelectGroup;

/** Stands in for the empty value, which Radix reserves for "not chosen". */
const EMPTY_VALUE = "__vibessh_select_empty__";

function isGroup(item: SelectItem): item is SelectGroup {
  return "options" in item;
}

interface SelectProps {
  value: string;
  onChange: (value: string) => void;
  items: SelectItem[];
  /** Shown when `value` matches no option - the native element's empty state. */
  placeholder?: string;
  disabled?: boolean;
  /** Applied to the trigger, so callers can size it the way they sized the
   * `<select>` this replaced. */
  className?: string;
  "aria-label"?: string;
  id?: string;
  name?: string;
}

/**
 * The app's dropdown.
 *
 * **Why this is not a `<select>`.** A native select's option list is drawn by
 * the operating system, not the page: on Windows it is a white menu with the
 * system's own highlight, in the middle of a dark application, and no
 * stylesheet can reach it. That is the entire reason this exists - everything
 * else about a native select was fine.
 *
 * Built on Radix's Select primitive rather than by hand, which is a
 * deliberate exception in a codebase that hand-builds its controls. A
 * listbox is the one primitive where doing it yourself is usually subtly
 * wrong: keyboard navigation, type-ahead, `aria-activedescendant`, portalled
 * positioning that avoids the screen edge, scroll locking, and returning
 * focus to the trigger on close. `Tooltip` and `Switch` here are an
 * afternoon's work; this is not.
 *
 * The API stays close to the element it replaces - a value, a change
 * handler, a flat list or groups - so migrating a call site is a mechanical
 * edit rather than a rewrite.
 */
export function Select({ value, onChange, items, placeholder, disabled, className, id, name, ...rest }: SelectProps) {
  // Radix reads an empty value as "nothing chosen yet" and draws the
  // placeholder, so an option that *is* the empty choice - "No role", "Do not
  // move the databases" - would leave the trigger blank while sitting ticked
  // in the list. Where the caller lists such an option, it travels under a
  // sentinel so Radix sees a real value and shows its label; the caller still
  // gets "" back.
  const hasEmptyOption = items.some((item) => (isGroup(item) ? item.options.some((o) => o.value === "") : item.value === ""));
  const encode = (raw: string) => (hasEmptyOption && raw === "" ? EMPTY_VALUE : raw);
  const decode = (raw: string) => (raw === EMPTY_VALUE ? "" : raw);

  return (
    <RadixSelect.Root value={encode(value)} onValueChange={(next) => onChange(decode(next))} disabled={disabled} name={name}>
      <RadixSelect.Trigger className={`select-trigger ${className ?? ""}`.trim()} id={id} aria-label={rest["aria-label"]}>
        <RadixSelect.Value className="select-value" placeholder={placeholder} />
        <RadixSelect.Icon className="select-trigger-icon">
          <Icon name="chevron-down" size={14} />
        </RadixSelect.Icon>
      </RadixSelect.Trigger>

      <RadixSelect.Portal>
        {/* `position="popper"` so the list sits below the trigger and flips
            when it would run off the screen, rather than covering the
            control the way the default aligned position does. */}
        <RadixSelect.Content className="select-content" position="popper" sideOffset={4}>
          <RadixSelect.ScrollUpButton className="select-scroll">
            <Icon name="chevron-up" size={12} />
          </RadixSelect.ScrollUpButton>

          <RadixSelect.Viewport className="select-viewport">
            {items.map((item, index) =>
              isGroup(item) ? (
                <RadixSelect.Group key={`${item.label}-${index}`}>
                  <RadixSelect.Label className="select-group-label">{item.label}</RadixSelect.Label>
                  {item.options.map((option) => (
                    <Option key={option.value} option={option} value={encode(option.value)} />
                  ))}
                </RadixSelect.Group>
              ) : (
                <Option key={item.value} option={item} value={encode(item.value)} />
              ),
            )}
          </RadixSelect.Viewport>

          <RadixSelect.ScrollDownButton className="select-scroll">
            <Icon name="chevron-down" size={12} />
          </RadixSelect.ScrollDownButton>
        </RadixSelect.Content>
      </RadixSelect.Portal>
    </RadixSelect.Root>
  );
}

function Option({ option, value }: { option: SelectOption; value: string }) {
  return (
    <RadixSelect.Item className="select-item" value={value} disabled={option.disabled}>
      <RadixSelect.ItemText>{option.label}</RadixSelect.ItemText>
      {/* The tick marks the current value. Reserved space rather than shown
          only when selected, so the labels do not shift as the highlight
          moves down the list. */}
      <span className="select-item-check">
        <RadixSelect.ItemIndicator>
          <Icon name="check" size={13} />
        </RadixSelect.ItemIndicator>
      </span>
    </RadixSelect.Item>
  );
}
