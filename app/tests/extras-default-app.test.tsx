import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PRODUCT_NAME } from "../src/config/product";
import { useUiStore } from "../src/stores/ui";
import type { ExtrasFakeOptions } from "./support/extras";
import { makeAccount, mockCore } from "./support/core";
import { renderApp } from "./support/render";

function setup(extras: ExtrasFakeOptions, failures = {}, language: "en" | "de" = "en") {
  const core = mockCore({ accounts: [makeAccount()], extras, failures, preferences: { theme: "system", language } });
  useUiStore.setState({ view: { kind: "settings", section: "general" } });
  return { core, ...renderApp(language) };
}

const section = () => screen.findByRole("region", { name: /Default mail app|Standard-Mail-App/ });

describe("default mail app", () => {
  it("macOS: says it is not the default and makes it the default on a click", async () => {
    const { core, user } = setup({ defaultMailApp: { isDefault: false, direct: true }, setDefaultOutcome: "done" });
    const block = await section();
    expect((await within(block).findByTestId("default-app-state")).textContent).toBe("Another app opens email links.");
    expect(core.callsOf("set_default_mail_app")).toEqual([]);

    await user.click(within(block).getByRole("button", { name: `Make ${PRODUCT_NAME} the default` }));
    expect((await within(block).findByTestId("default-app-outcome")).textContent).toBe(`Done: email links now open in ${PRODUCT_NAME}.`);
    await waitFor(() => {
      expect(within(block).getByTestId("default-app-state").dataset["state"]).toBe("yes");
    });
    expect(within(block).queryByTestId("default-app-set")).toBeNull();
    expect(core.callsOf("set_default_mail_app")).toHaveLength(1);
  });

  it("macOS: when the system refuses, explains the way through Mail's settings and checks again on request", async () => {
    const { core, user } = setup({ defaultMailApp: { isDefault: false, direct: true }, setDefaultOutcome: "manual" });
    const block = await section();
    await user.click(await within(block).findByRole("button", { name: `Make ${PRODUCT_NAME} the default` }));
    const outcome = await within(block).findByTestId("default-app-outcome");
    expect(outcome.getAttribute("role")).toBe("status");
    expect(outcome.textContent).toContain("open Mail, choose Mail, Settings, General");
    const asked = core.callsOf("get_default_mail_app").length;
    await user.click(within(block).getByRole("button", { name: "Check again" }));
    await waitFor(() => {
      expect(core.callsOf("get_default_mail_app").length).toBe(asked + 1);
    });
  });

  it("Windows: cannot tell, opens the system's settings and says what to pick there", async () => {
    const { user } = setup({ defaultMailApp: { isDefault: null, direct: false }, setDefaultOutcome: "openedSettings" });
    const block = await section();
    expect((await within(block).findByTestId("default-app-state")).dataset["state"]).toBe("unknown");
    // Windows never says, so checking again would not help.
    expect(within(block).queryByRole("button", { name: "Check again" })).toBeNull();
    await user.click(within(block).getByRole("button", { name: "Open Windows settings" }));
    expect((await within(block).findByTestId("default-app-outcome")).textContent).toContain(
      `Choose ${PRODUCT_NAME} for email links (MAILTO) there.`,
    );
  });

  it("already the default: no button", async () => {
    setup({ defaultMailApp: { isDefault: true, direct: true } }, {}, "de");
    const block = await section();
    expect((await within(block).findByTestId("default-app-state")).textContent).toBe(`${PRODUCT_NAME} ist deine Standard-Mail-App.`);
    expect(within(block).queryByTestId("default-app-set")).toBeNull();
  });

  it("a failure to check is shown with a way to try again", async () => {
    const { core, user } = setup({}, { get_default_mail_app: "command get_default_mail_app not allowed" });
    const block = await section();
    const error = await within(block).findByTestId("default-app-error");
    expect(error.dataset["errorCode"]).toBe("internal");
    expect(error.textContent).toContain(`Whether ${PRODUCT_NAME} is the default could not be checked`);
    await user.click(within(error).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(core.callsOf("get_default_mail_app").length).toBeGreaterThan(1);
    });
  });
});
