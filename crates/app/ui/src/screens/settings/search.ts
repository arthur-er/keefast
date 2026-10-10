// What a settings page shows for a search: General and Vault settings C filter their sections as
// the user types, and the "On this page" index counts the matches in each.

/** One searchable thing on a settings page: a row, a note, a block. */
export interface SearchableItem {
  id: string;
  /** What a search matches: the label, the description and keywords. */
  search: string;
  /** Shown whatever the search, such as a form being filled in. */
  pinned?: boolean;
}

export interface SearchableSection<Item extends SearchableItem = SearchableItem> {
  id: string;
  name: string;
  desc?: string;
  /** The section's cards, each a list of items. */
  cards: Item[][];
}

type ItemOf<Section extends SearchableSection> = Section["cards"][number][number];

export interface SectionMatch<Section extends SearchableSection> {
  section: Section;
  /** The cards with the items to show; cards left with none are left out. */
  cards: ItemOf<Section>[][];
  /** How many matches the index shows: the items matched, and one for the section's title. */
  count: number;
}

export interface SearchResult<Section extends SearchableSection> {
  /** Every section, in order; those with nothing to show have `count` 0 and no cards. */
  sections: SectionMatch<Section>[];
  /** Whether the search matched nothing at all. */
  empty: boolean;
}

/** Filters `sections` for `query`. A section whose name or description matches shows whole;
 * otherwise only its matching (and pinned) items show. An empty query shows everything. */
export function searchSections<Section extends SearchableSection>(
  sections: readonly Section[],
  query: string,
): SearchResult<Section> {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return {
      sections: sections.map((section) => ({ section, cards: section.cards.filter((card) => card.length > 0), count: 0 })),
      empty: false,
    };
  }
  const matched = sections.map((section): SectionMatch<Section> => {
    const titleHit = `${section.name} ${section.desc ?? ""}`.toLowerCase().includes(needle);
    let hits = 0;
    let pinned = false;
    const cards = section.cards
      .map((card) =>
        card.filter((item: ItemOf<Section>) => {
          const hit = item.search.toLowerCase().includes(needle);
          if (hit) hits += 1;
          if (item.pinned) pinned = true;
          return titleHit || hit || item.pinned;
        }),
      )
      .filter((card) => card.length > 0);
    const count = hits + (titleHit ? 1 : 0) || (pinned ? 1 : 0);
    return { section, cards: count ? cards : [], count };
  });
  return { sections: matched, empty: matched.every((match) => match.count === 0) };
}
