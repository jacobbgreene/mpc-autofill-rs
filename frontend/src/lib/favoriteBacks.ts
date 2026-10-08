import type { ArtworkChoice } from "./artwork";

const storageKey = "mpc-autofill.favorite-backs";

export function favoriteBacks(): ArtworkChoice[] {
  try {
    const stored = JSON.parse(localStorage.getItem(storageKey) ?? "[]");
    if (!Array.isArray(stored)) return [];
    return stored.filter(
      (choice): choice is ArtworkChoice =>
        typeof choice?.art?.drive_id === "string" &&
        typeof choice.art.name === "string" &&
        typeof choice.art.label === "string" &&
        typeof choice.query === "string" &&
        typeof choice.index === "number" &&
        typeof choice.count === "number",
    );
  } catch {
    return [];
  }
}

export function initialCardBack(
  savedDraft: { common_back: ArtworkChoice | null } | null,
): ArtworkChoice | null {
  return savedDraft ? savedDraft.common_back : favoriteBacks()[0] ?? null;
}

export function cardBackChanged(
  selected: ArtworkChoice | null,
  initialId: string | null,
): boolean {
  return (selected?.art.drive_id ?? null) !== initialId;
}

export function toggleFavoriteBack(choice: ArtworkChoice): ArtworkChoice[] {
  const current = favoriteBacks();
  const exists = current.some((item) => item.art.drive_id === choice.art.drive_id);
  const next = exists
    ? current.filter((item) => item.art.drive_id !== choice.art.drive_id)
    : [choice, ...current];
  localStorage.setItem(storageKey, JSON.stringify(next));
  window.dispatchEvent(new Event("favorite-backs-changed"));
  return next;
}
