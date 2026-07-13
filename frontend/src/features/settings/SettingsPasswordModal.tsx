import { useEffect, useRef, useState, type FormEvent } from "react";
import { KeyRound, LoaderCircle } from "lucide-react";

import type { VerifySettingsPassword } from "./settingsTypes";

export interface SettingsPasswordModalProps {
  open: boolean;
  onCancel: () => void;
  onUnlock: (password: string) => void;
  verify: VerifySettingsPassword;
}

export function SettingsPasswordModal({
  open,
  onCancel,
  onUnlock,
  verify,
}: SettingsPasswordModalProps) {
  const dialogRef = useRef<HTMLElement | null>(null);
  const passwordInputRef = useRef<HTMLInputElement | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [isVerifying, setIsVerifying] = useState(false);

  useEffect(() => {
    if (!open) {
      return;
    }

    previousFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    passwordInputRef.current?.focus();
    return () => {
      const previousFocus = previousFocusRef.current;
      previousFocusRef.current = null;
      if (previousFocus?.isConnected) {
        previousFocus.focus();
      }
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !isVerifying) {
        setPassword("");
        setError("");
        onCancel();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }

      const focusable = Array.from(
        dialogRef.current?.querySelectorAll<HTMLElement>(
          'input:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
      const first = focusable.at(0);
      const last = focusable.at(-1);
      if (!first || !last) {
        return;
      }
      const activeElement = document.activeElement;
      if (
        event.shiftKey &&
        (activeElement === first || !dialogRef.current?.contains(activeElement))
      ) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isVerifying, onCancel, open]);

  if (!open) {
    return null;
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!password) {
      setError("Enter the settings password.");
      return;
    }

    setIsVerifying(true);
    setError("");
    try {
      const verified = await verify(password);
      if (!verified) {
        setError("Incorrect password");
        return;
      }

      const unlockedPassword = password;
      setPassword("");
      onUnlock(unlockedPassword);
    } catch {
      setError("The settings password could not be verified.");
    } finally {
      setIsVerifying(false);
    }
  }

  function handleCancel() {
    setPassword("");
    setError("");
    onCancel();
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4">
      <section
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-password-title"
        aria-describedby="settings-password-description"
        className="w-full max-w-[320px] rounded-md border border-[#454542] bg-[#292928] p-4 text-white shadow-2xl"
      >
        <div className="flex justify-center text-[#8dc7ef]">
          <KeyRound className="h-5 w-5" aria-hidden="true" />
        </div>
        <h2
          id="settings-password-title"
          className="mt-2 text-center text-[12pt] font-semibold leading-tight"
        >
          Advanced
        </h2>
        <p
          id="settings-password-description"
          className="mt-2 text-center text-[8pt] leading-snug text-[#d8d2c8]"
        >
          Enter password
        </p>

        <form className="mt-4" onSubmit={(event) => void handleSubmit(event)}>
          <label htmlFor="settings-password" className="block text-[8pt] font-semibold text-[#d8d2c8]">
            Password
          </label>
          <input
            ref={passwordInputRef}
            id="settings-password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(event) => {
              setPassword(event.target.value);
              setError("");
            }}
            aria-invalid={Boolean(error)}
            aria-describedby={error ? "settings-password-error" : undefined}
            disabled={isVerifying}
            className="mt-1 h-9 w-full rounded border border-[#454542] bg-[#1f1f1e] px-2 text-[9pt] text-white outline-none focus:border-[#1f74ae] focus:ring-2 focus:ring-cyan-200/25 disabled:opacity-70"
          />
          {error ? (
            <div
              id="settings-password-error"
              role="alert"
              className="mt-1.5 text-[7.5pt] leading-tight text-[#f4b1a9]"
            >
              {error}
            </div>
          ) : null}

          <div className="mt-4 grid grid-cols-2 gap-2">
            <button
              type="button"
              onClick={handleCancel}
              disabled={isVerifying}
              className="inline-flex min-h-9 items-center justify-center rounded-md bg-[#3a3a38] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#454542] disabled:cursor-not-allowed disabled:opacity-60"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isVerifying}
              className="inline-flex min-h-9 items-center justify-center gap-1.5 rounded-md bg-[#1f74ae] px-3 py-2 text-[9pt] font-semibold text-white shadow-sm transition hover:bg-[#2874a8] disabled:cursor-wait disabled:opacity-70"
            >
              {isVerifying ? <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" /> : null}
              {isVerifying ? "Unlocking..." : "Unlock"}
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}
