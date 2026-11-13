// What sits under the title in Entry editor A: the colour and expiry chips, and the tags.

import { useMemo, useRef, useState } from "react";
import { CalendarClock, CalendarDays, Check, CircleDashed, ClockAlert, Plus, X } from "lucide-react";

import type { Color } from "@/bindings";
import {
  Badge, colorVar, Menu, MenuContent, MenuItem, MenuLabel, MenuTrigger, Popover, PopoverAnchor, PopoverContent,
  PopoverTrigger, Switch,
} from "@/kit";
import { cn } from "@/lib/utils";
import { COLOR_NAMES } from "@/shell/Sidebar";

import { addTags, dateIn, ERROR_TEXT } from "./draft";

const COLORS: Color[] = ["red", "orange", "yellow", "green", "blue", "violet"];

export function ColorChip({ color, onChange }: { color: Color | null; onChange: (color: Color | null) => void }) {
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="ee-chip" aria-label={`Colour: ${color ? COLOR_NAMES[color] : "none"}`}>
          {color ? (
            <>
              <span className="dot" style={{ background: colorVar(color) }} />
              {COLOR_NAMES[color]}
            </>
          ) : (
            <>
              <CircleDashed />
              Colour
            </>
          )}
        </button>
      </MenuTrigger>
      <MenuContent className="w-[200px]">
        <MenuLabel>Colour</MenuLabel>
        <MenuItem icon={<CircleDashed />} onSelect={() => onChange(null)}>
          None
          {!color && <Check className="ml-auto" />}
        </MenuItem>
        {COLORS.map((candidate) => (
          <MenuItem key={candidate} icon={<span className="dot mx-[3px] h-2.5 w-2.5" style={{ background: colorVar(candidate) }} />} onSelect={() => onChange(candidate)}>
            {COLOR_NAMES[candidate]}
            {color === candidate && <Check className="ml-auto" />}
          </MenuItem>
        ))}
      </MenuContent>
    </Menu>
  );
}

export function ExpiryChip({
  expires,
  expiry,
  expired,
  invalid,
  onChange,
}: {
  expires: boolean;
  expiry: string;
  expired: boolean;
  invalid: boolean;
  onChange: (change: { expires?: boolean; expiry?: string }) => void;
}) {
  const field = useRef<HTMLInputElement>(null);
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button type="button" className={cn("ee-chip", invalid && "is-invalid")} aria-label="Expires">
          <CalendarClock />
          <span>{expires ? expiry || "YYYY-MM-DD" : "No expiry"}</span>
          {expired && (
            <Badge tone="danger">
              <ClockAlert className="ico-sm" />
              Expired
            </Badge>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-[280px]" aria-label="Expires" onOpenAutoFocus={(event) => {
        event.preventDefault();
        field.current?.focus();
      }}>
        <div className="ee-pop-head">
          <b>Expires</b>
          <span className="spacer" />
          <Switch
            label="Expires"
            checked={expires}
            onCheckedChange={(on) => {
              onChange({ expires: on, ...(on && !expiry ? { expiry: dateIn(0) } : {}) });
              if (on) requestAnimationFrame(() => field.current?.focus());
            }}
          />
        </div>
        <div className="vstack px-3 pt-0.5 pb-3">
          <label className={cn("input mono", invalid && "invalid", !expires && "opacity-45")}>
            <CalendarDays className="ico-sm" />
            <input
              ref={field}
              value={expiry}
              placeholder="YYYY-MM-DD"
              aria-label="Expiry date"
              disabled={!expires}
              spellCheck={false}
              onChange={(event) => onChange({ expiry: event.target.value })}
            />
          </label>
          {invalid ? <div className="error-msg">{ERROR_TEXT.expiry}</div> : <div className="hint">At the start of that day, UTC.</div>}
          <div className="hstack mt-0.5 gap-1">
            {(
              [
                ["In 30 days", 30],
                ["In 90 days", 90],
                ["In a year", 365],
              ] as const
            ).map(([label, days]) => (
              <button key={days} type="button" className="btn secondary sm" onClick={() => onChange({ expires: true, expiry: dateIn(days) })}>
                {label}
              </button>
            ))}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** Tags as tokens, each in its colour in `colorOf`, typed with commas or Enter, the Vaults' tags
 * offered as the user types. */
export function TagInput({
  tags,
  known,
  colorOf,
  onChange,
}: {
  tags: string[];
  known: readonly string[];
  colorOf: (tag: string) => Color;
  onChange: (tags: string[]) => void;
}) {
  const [text, setText] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const query = text.trim().toLowerCase();
  const offered = useMemo(() => {
    if (!query) return [];
    const matching = known.filter((tag) => !tags.includes(tag) && tag.toLowerCase().includes(query)).slice(0, 6);
    const exact = known.some((tag) => tag.toLowerCase() === query) || tags.some((tag) => tag.toLowerCase() === query);
    return [...matching.map((tag) => ({ tag, create: false })), ...(exact ? [] : [{ tag: text.trim(), create: true }])];
  }, [query, known, tags, text]);
  const add = (value: string) => {
    onChange(addTags(tags, value));
    setText("");
    setActive(0);
  };
  return (
    <Popover open={offered.length > 0}>
      <PopoverAnchor asChild>
        <div className="ee-tok" onClick={(event) => event.target === event.currentTarget && input.current?.focus()}>
          {tags.map((tag, index) => (
            <span key={tag} className="ee-tk">
              <span className="dot" style={{ background: colorVar(colorOf(tag)) }} />
              {tag}
              <button type="button" aria-label={`Remove ${tag}`} title={`Remove ${tag}`} onClick={() => onChange(tags.filter((_, at) => at !== index))}>
                <X />
              </button>
            </span>
          ))}
          <input
            ref={input}
            value={text}
            aria-label="Add tag"
            placeholder={tags.length ? "Add tag" : "Separate tags with commas"}
            onChange={(event) => {
              const value = event.target.value;
              if (/[,;]/.test(value)) add(value);
              else {
                setText(value);
                setActive(0);
              }
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" && text.trim()) {
                event.preventDefault();
                add(offered[active]?.tag ?? text);
              } else if (event.key === "Backspace" && !text && tags.length) {
                onChange(tags.slice(0, -1));
              } else if ((event.key === "ArrowDown" || event.key === "ArrowUp") && offered.length) {
                event.preventDefault();
                setActive((at) => (at + (event.key === "ArrowDown" ? 1 : offered.length - 1)) % offered.length);
              } else if (event.key === "Escape" && text) {
                event.preventDefault();
                setText("");
              }
            }}
            onBlur={() => text.trim() && add(text)}
          />
        </div>
      </PopoverAnchor>
      <PopoverContent
        className="menu w-[220px] p-1"
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
        onInteractOutside={(event) => {
          if (event.target instanceof Node && input.current?.closest(".ee-tok")?.contains(event.target)) event.preventDefault();
        }}
      >
        {offered.map((option, index) => (
          <div
            key={option.tag}
            role="option"
            aria-selected={index === active}
            className={cn("menu-item", index === active && "is-active")}
            onMouseDown={(event) => {
              event.preventDefault();
              add(option.tag);
            }}
            onMouseEnter={() => setActive(index)}
          >
            {option.create ? (
              <>
                <Plus />
                Create tag “{option.tag}”
              </>
            ) : (
              <>
                <span className="dot mx-[3px] h-2 w-2" style={{ background: colorVar(colorOf(option.tag)) }} />
                {option.tag}
              </>
            )}
          </div>
        ))}
      </PopoverContent>
    </Popover>
  );
}
