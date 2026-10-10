// The page General and Vault settings C share: a toolbar with where the user is and the search
// (`/`), then one long page of sections beside the "On this page" index, which follows the
// scrolling and counts each section's matches while searching. Fills the main area right of the
// sidebar (`ShellState.page`).

import { createContext, Fragment, type ReactNode, useCallback, useContext, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, Search, SearchX, X } from "lucide-react";

import { Button, Kbd } from "@/kit";
import { highlight } from "@/lib/format";
import { SHORTCUTS } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { useAppCommand } from "@/shell/commands";
import { useKeysActive } from "@/shell/hooks";

import { type SearchableItem, type SearchableSection, searchSections } from "./search";

export interface SettingsItem extends SearchableItem {
  render: () => ReactNode;
}

export interface SettingsSection extends SearchableSection<SettingsItem> {
  /** Shown after the title, such as how many browsers are paired. */
  count?: number;
}

/** How far above a section jumping to it leaves it. */
const JUMP_MARGIN = 28;

const QueryContext = createContext("");

/** What the user searches for, trimmed. */
export function useSearchQuery(): string {
  return useContext(QueryContext);
}

/** `text` with the search's match marked. */
export function Hl({ children }: { children: string }) {
  const query = useContext(QueryContext);
  const parts = highlight(children, query);
  if (!parts) return <>{children}</>;
  return (
    <>
      {parts.before}
      <mark>{parts.match}</mark>
      {parts.after}
    </>
  );
}

/** `.set-row`: a label and description, the control on the right. */
export function SettingRow({
  label,
  desc,
  children,
  className,
}: {
  label: string;
  desc?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("set-row", className)}>
      <div className="txt">
        <div className="lbl">
          <Hl>{label}</Hl>
        </div>
        {desc !== undefined && desc !== null && <div className="desc">{typeof desc === "string" ? <Hl>{desc}</Hl> : desc}</div>}
      </div>
      {children !== undefined && <div className="ctl">{children}</div>}
    </div>
  );
}

export function SettingsLayout({
  crumbs,
  searchLabel,
  title,
  sub,
  banner,
  sections,
  tip,
  emptyLabel,
  section,
}: {
  /** Where the user is, in the toolbar. */
  crumbs: ReactNode;
  /** The search field's placeholder: "Search settings". */
  searchLabel: string;
  title: string;
  sub: ReactNode;
  /** Above the title, such as the config error. */
  banner?: ReactNode;
  sections: readonly SettingsSection[];
  /** Under the index, after the search tip. */
  tip: string;
  /** "settings" in "No settings match". */
  emptyLabel: string;
  /** The section to show first. */
  section?: string;
}) {
  const [query, setQuery] = useState("");
  const result = useMemo(() => searchSections(sections, query), [sections, query]);
  const searching = query.trim() !== "";
  const scroller = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const [current, setCurrent] = useState<string | undefined>(sections[0]?.id);
  const keysActive = useKeysActive();

  useAppCommand(
    { id: "searchSettings", title: `${searchLabel}`, section: "Go to", icon: Search, shortcut: SHORTCUTS.search, run: () => field.current?.focus() },
    keysActive,
  );

  const sectionElement = (id: string) => scroller.current?.querySelector<HTMLElement>(`[data-sec="${CSS.escape(id)}"]`) ?? null;

  /** The section at the top of the page, or the last when scrolled to the end. */
  const spy = useCallback(() => {
    const box = scroller.current;
    if (!box) return;
    const shown = [...box.querySelectorAll<HTMLElement>(".set-section")];
    const top = box.getBoundingClientRect().top + 80;
    let at = shown[0]?.dataset.sec;
    for (const element of shown) if (element.getBoundingClientRect().top <= top) at = element.dataset.sec;
    if (box.scrollTop + box.clientHeight >= box.scrollHeight - 4) at = shown[shown.length - 1]?.dataset.sec;
    setCurrent(at);
  }, []);

  const jump = useCallback((id: string, smooth = true) => {
    const box = scroller.current;
    const element = sectionElement(id);
    if (!box || !element) return;
    const top = box.scrollTop + element.getBoundingClientRect().top - box.getBoundingClientRect().top - JUMP_MARGIN;
    box.scrollTo({ top, behavior: smooth ? "smooth" : "instant" });
  }, []);

  // Opened at a section (a "Manage" link, the master key): start there, once it is drawn (the
  // page's data may still be on its way).
  const jumped = useRef<string | undefined>(undefined);
  const ready = sections.length > 0;
  useLayoutEffect(() => {
    if (!section || !ready || jumped.current === section) return;
    jumped.current = section;
    jump(section, false);
    spy();
  }, [section, ready, jump, spy]);

  useLayoutEffect(spy, [result, spy]);

  const search = (next: string) => {
    setQuery(next);
    scroller.current?.scrollTo({ top: 0, behavior: "instant" });
  };

  return (
    <QueryContext.Provider value={query.trim()}>
      <div className="vc-main">
        <div className="toolbar">
          <span className="crumbs min-w-0">{crumbs}</span>
          <div className="search">
            <Search className="ico-sm" />
            <input
              ref={field}
              value={query}
              placeholder={searchLabel}
              aria-label={searchLabel}
              spellCheck={false}
              onChange={(event) => search(event.target.value)}
              onKeyDown={(event) => {
                if (event.key !== "Escape") return;
                // Escape leaves the search, and only the next one leaves the page.
                event.preventDefault();
                search("");
                field.current?.blur();
              }}
            />
            {query ? (
              <button
                type="button"
                className="btn sm ghost icon"
                aria-label="Clear search"
                onClick={() => {
                  search("");
                  field.current?.focus();
                }}
              >
                <X className="ico-sm" />
              </button>
            ) : (
              <Kbd combo="/" />
            )}
          </div>
        </div>
        <div className="vc-scroll" ref={scroller} onScroll={spy}>
          <div className="vc-wrap">
            <div>
              {banner}
              <h1 className="vc-title">{title}</h1>
              <p className="vc-sub">{sub}</p>
              {result.sections.map(({ section, cards, count }) =>
                searching && count === 0 ? null : (
                  <section key={section.id} className="set-section" data-sec={section.id} aria-labelledby={`set-${section.id}`}>
                    <div className="set-head">
                      <h2 id={`set-${section.id}`}>
                        <Hl>{section.name}</Hl>
                        {section.count !== undefined && <span className="n">{section.count}</span>}
                      </h2>
                      {section.desc && (
                        <p>
                          <Hl>{section.desc}</Hl>
                        </p>
                      )}
                    </div>
                    {cards.map((card, index) => (
                      <div key={index} className="set-card">
                        {card.map((item) => (
                          <Fragment key={item.id}>{item.render()}</Fragment>
                        ))}
                      </div>
                    ))}
                  </section>
                ),
              )}
              {searching && result.empty && (
                <div className="vc-empty">
                  <SearchX />
                  <div>
                    No {emptyLabel} match “<b>{query.trim()}</b>”
                  </div>
                  <Button size="sm" onClick={() => search("")}>
                    Clear search
                  </Button>
                </div>
              )}
            </div>
            <nav className="vc-index" aria-label="On this page">
              <div className="ih">On this page</div>
              {result.sections.map(({ section, count }) => (
                <a
                  key={section.id}
                  href={`#${section.id}`}
                  className={cn(current === section.id && "is-on", searching && count === 0 && "dim")}
                  aria-current={current === section.id ? "location" : undefined}
                  onClick={(event) => {
                    event.preventDefault();
                    jump(section.id);
                  }}
                >
                  {section.name}
                  {searching && count > 0 && <span className="n">{count}</span>}
                </a>
              ))}
              <div className="itip">
                Press <Kbd combo="/" /> to search settings.
                <br />
                {tip}
              </div>
            </nav>
          </div>
        </div>
      </div>
    </QueryContext.Provider>
  );
}

/** The crumbs' separator. */
export function CrumbSep() {
  return <ChevronRight className="ico-sm subtle" />;
}
