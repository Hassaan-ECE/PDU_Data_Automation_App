import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { App } from "@/app/App";

describe("App", () => {
  it("renders the operator panel shell", () => {
    render(<App />);

    expect(screen.getByText("00:00:00")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "208V System" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "415V System" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Breaker Burn-In" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Select Test Unit...")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Transformer SN...")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Browse unit folder" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reset Panel" })).toBeInTheDocument();
    expect(screen.getByText("208V Transformer Check")).toBeInTheDocument();
    expect(screen.getByText("System Burn-In")).toBeInTheDocument();
  });
});
