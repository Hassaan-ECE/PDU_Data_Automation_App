# Advanced Settings Page Separation Design

**Status:** Approved UI direction on 2026-07-14
**Branch:** `fix/v0.2.15-corrective`
**Release operation:** Out of scope

## Purpose

Replace the combined **Station & Teams** Advanced page with two focused sibling pages so operators and administrators can understand which settings affect identity/floor synchronization and which settings control Teams delivery.

## Advanced menu

The password-locked Advanced menu exposes:

1. **Station & Identities**
2. **Teams & Notifications**
3. **Change password**

The open operator pages remain unchanged. Both new pages remain behind the existing Advanced password and use the existing Back/unsaved-changes behavior.

## Station & Identities page

This page contains only:

- searchable **This PC identity** selection;
- searchable/addable **Manage identities** control;
- immutable stable-id and Floor/Admin role details;
- **Use on this PC** for a staged identity creation;
- shared OneDrive folder Browse/Clear controls;
- Existing floor password during Connect;
- floor synchronization status.

Creating or renaming identities remains possible only with a configured shared floor. A path change uses the existing `connect` or `local` behavior. An ordinary identity save changes identity/catalog data and this PC's local identity without rewriting Teams policy.

## Teams & Notifications page

This page contains only:

- Teams destination display name;
- masked Teams Workflow/webhook URL;
- Notifications enabled master toggle;
- Changeover-card toggle;
- Send test ping;
- saved floor-sync status as context.

Saving this page changes only Teams/notification policy. It does not rewrite the identity catalog, this PC identity, or shared-folder pointer.

## Independent save scopes

The existing broad `advanced` scope is split for these pages:

- `identity`: apply explicit identity renames/creation and local `station_id`; do not apply webhook, destination, or event toggles.
- `teams`: apply enabled state, destination, webhook replacement, idle timeout, and Advanced event toggles; do not apply catalog names or local `station_id`.
- `connect`: unchanged path connect/adopt/seed behavior, including the floor password.
- `local`: unchanged shared-path clearing behavior.
- `operator`: unchanged shifts/Main/included/summary behavior.

Both shared mutations keep the existing floor read-modify-write lock. Omitted identities are never deletions, an empty webhook preserves the existing secret, and notification/floor failures remain soft failures for automation.

## State and navigation

- `SettingsView` gains separate identity and Teams views.
- Entering either page starts from the same authoritative merged settings snapshot.
- Dirty state is page-aware. Typing/selecting/staging identity data marks only the identity page dirty; Teams edits mark only the Teams page dirty.
- The 45-second Settings poll applies only while the current form is clean.
- Successful saves replace the local form with the backend response.
- Test ping remains disabled until Teams edits are saved.

## Compatibility

- No settings-file schema change is introduced by this UI separation.
- Existing schema 1 and schema 2 floors remain readable.
- Existing local settings and shared-folder pointers remain valid.
- No version bump, installer, updater, tag, push, or release is part of this change.

## Verification

Automated tests must prove:

- Advanced displays two separate menu items and no combined Station & Teams item;
- both pages remain password-locked;
- Station & Identities renders identity and folder controls but no webhook/Teams toggles;
- Teams & Notifications renders Teams controls but no identity/folder editors;
- identity saves send `identity` scope and cannot alter Teams fields;
- Teams saves send `teams` scope and cannot alter identity/catalog fields;
- Connect/local/operator scopes retain their current behavior;
- dirty navigation and clean-form polling still work on both pages;
- identity creation, generated-id adoption, role filtering, Changeover toggle, and test ping continue to work.

Manual hot-reload verification should confirm the two Advanced menu entries and page contents in the already-running `bun run desktop` app. Live OneDrive, Teams delivery, installer, and multi-PC behavior are not established by this visual check.

## Non-goals

- A third shared-sync page.
- Tabs inside one combined page.
- Changes to operator-open shift/summary access.
- Identity deletion or role conversion.
- Notification event redesign beyond moving the existing controls.
