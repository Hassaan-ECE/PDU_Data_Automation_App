const DEFAULT_OPERATOR_NAMES = ["Sean", "Long", "Jose"];
const OPERATOR_NAMES_STORAGE_KEY = "pdu.operatorNames";

export function operatorNameKey(name: string) {
  return name.trim().toLowerCase();
}

export function normalizeOperatorNames(values: unknown): string[] {
  if (!Array.isArray(values)) {
    return [...DEFAULT_OPERATOR_NAMES];
  }

  const seen = new Set<string>();
  const names: string[] = [];

  for (const value of values) {
    if (typeof value !== "string") {
      continue;
    }

    const trimmed = value.trim();
    const key = operatorNameKey(trimmed);

    if (!trimmed || seen.has(key)) {
      continue;
    }

    seen.add(key);
    names.push(trimmed);
  }

  return names;
}

export function storeOperatorNames(names: string[]) {
  const normalized = normalizeOperatorNames(names);

  try {
    window.localStorage.setItem(OPERATOR_NAMES_STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // localStorage can be unavailable in restricted browser contexts.
  }

  return normalized;
}

export function loadOperatorNames() {
  try {
    const stored = window.localStorage.getItem(OPERATOR_NAMES_STORAGE_KEY);

    if (stored === null) {
      return storeOperatorNames([...DEFAULT_OPERATOR_NAMES]);
    }

    const parsed = JSON.parse(stored) as unknown;
    const normalized = normalizeOperatorNames(parsed);

    if (JSON.stringify(parsed) !== JSON.stringify(normalized)) {
      storeOperatorNames(normalized);
    }

    return normalized;
  } catch {
    return storeOperatorNames([...DEFAULT_OPERATOR_NAMES]);
  }
}

export function addOperatorName(names: string[], name: string) {
  const normalized = normalizeOperatorNames(names);
  const trimmed = name.trim();
  const key = operatorNameKey(trimmed);

  if (!trimmed || normalized.some((operatorName) => operatorNameKey(operatorName) === key)) {
    return normalized;
  }

  return [...normalized, trimmed];
}

export function matchingOperatorNames(names: string[], query: string) {
  const key = operatorNameKey(query);

  if (!key) {
    return names;
  }

  const startsWith = names.filter((name) => operatorNameKey(name).startsWith(key));
  const contains = names.filter((name) => {
    const nameKey = operatorNameKey(name);

    return !nameKey.startsWith(key) && nameKey.includes(key);
  });

  return [...startsWith, ...contains];
}
