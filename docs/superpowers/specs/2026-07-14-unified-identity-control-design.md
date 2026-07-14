# Unified identity control and intentional floor creation

## Goal

Replace the duplicated **This PC identity** and **Manage identities** controls with one
Advanced-only identity control. The same control selects this PC's identity, adds a Floor
Station or Admin Identity, and starts an explicit rename of the selected identity.

An intentional identity save must also recover the current case where the configured shared
folder is reachable but `floor_settings.json` is absent. Ordinary loads and unrelated saves
must continue to avoid automatic reseeding.

## Confirmed behavior

- The control remains under password-locked **Advanced → Station & Identities**.
- Selecting an existing identity makes it this PC's local identity.
- Typing a unique name offers **Add as Floor Station** and **Add as Admin Identity**.
- A newly added identity is automatically selected for this PC.
- The selected existing identity has an explicit **Rename** action. Searching or typing by
  itself never renames an identity.
- Stable identity ids and roles remain immutable.
- Admin identities are visible in the Advanced identity picker so an admin PC can select one.
- Admin identities are excluded from every password-free operator surface, including Main
  poster choices, included-station rows, and shift summaries.

## Approaches considered

### Chosen: one combobox plus an explicit rename state

The existing identity combobox handles search, selection, and the two Add actions. Below the
selected identity, a compact metadata row shows its stable id and role with a **Rename** button.
Rename temporarily reveals one display-name input and Cancel action. This removes the permanent
second combobox while keeping rename intent unambiguous and accessible.

### Rejected: infer rename from free typing

Treating text edits as rename would make search queries destructive and make it difficult to
distinguish selection, creation, and rename.

### Rejected: separate identity-management modal

A modal would be clear but adds navigation and state for a small catalog. It is more UI
than this workflow needs.

## Missing floor-file behavior

The shared path on this PC resolves to:

```text
C:\Users\Syed.h.Shah\OneDrive - Delta Electronics, Inc\.PDU_Notifications
```

The folder is readable and contains `shift_log.json` plus `stations/`, but currently has no
`floor_settings.json`. The existing error is produced because Identity scope requires an
existing floor snapshot before applying a catalog patch.

An Identity-scope Save is an explicit policy mutation. While holding the existing floor lock:

1. Load the current floor file if present.
2. If absent, seed a floor snapshot from the current local cache and default historical catalog.
3. Apply the requested rename/create patch.
4. Validate and atomically write one `floor_settings.json`.
5. Apply the resulting catalog locally, including the generated id when a new identity is used
   on this PC.

This exception is limited to Identity scope. App startup, polling, Teams saves, operator saves,
and password changes do not recreate a missing floor file.

## Frontend state and flow

- `thisPcIdentityQuery` is the single combobox value.
- Selecting an existing entry clears staged creation/rename state and updates `station_id`.
- Choosing an Add option stages `catalog_create` with `select_for_this_pc: true`.
- The staged-new card shows the selected role and that the generated id will be assigned on Save.
- Rename mode stores the selected id and draft display name. Draft edits update only that catalog
  entry in the outgoing Identity request.
- Saving consumes the backend response so the generated id/name becomes the selected value.
- Dirty-form polling behavior remains unchanged: peer reloads never clobber staged identity work.

## Validation and failure behavior

- Existing display-name validation remains authoritative: trimmed, non-empty, at most 64
  characters, no control characters, and case-insensitive uniqueness.
- If the shared folder cannot be created/read/locked/written, Save fails without modifying the
  local selected identity or discarding the staged form.
- The webhook and settings password remain redacted from errors and tests.
- Notification failures and floor synchronization failures remain soft for automation and report
  processing.

## Verification

Backend tests prove:

- Identity create with a configured but missing floor file seeds and creates the identity under
  the read-modify-write lock.
- The new Admin identity is selected locally when requested.
- Ordinary load still does not reseed a missing floor file.
- Operator and Teams scopes retain their current missing-floor behavior.

Frontend tests prove:

- Station & Identities renders one identity combobox and no Manage identities combobox.
- Selecting an existing identity uses Identity scope.
- Adding an Admin or Floor identity sends `select_for_this_pc: true`.
- Rename requires the explicit Rename action and preserves the stable id.
- Admin identities appear in the Advanced picker but never in operator summary/Main controls.

Manual verification uses the configured `.PDU_Notifications` folder with no floor file: add an
Admin identity, Save, confirm `floor_settings.json` appears, and confirm the returned Admin
identity is selected on this PC.
