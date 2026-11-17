import { expect, test } from "@playwright/test";

const API_TOKEN = "playwright-api-token-0123456789-abcdef";

test("operator connects, approves and runs authenticated GPU chat", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Unlock local control plane")).toBeVisible();

  await page.getByPlaceholder("DREAD_API_TOKEN").fill(API_TOKEN);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("API CONNECTED")).toBeVisible();
  await expect(
    page.locator(".metric").filter({ hasText: "GPU supervisor" }),
  ).toContainText("Ready");
  await expect(page.locator("select")).toHaveValue("qwen3.8:27b");

  await page.locator("textarea").fill("Return the E2E mock response.");
  await page.getByRole("button", { name: "Create task" }).click();

  const approve = page.locator('button[title="Approve"]').first();
  await expect(approve).toBeVisible();
  await approve.click();

  const run = page.locator('button[title="Run"]').first();
  await expect(run).toBeVisible();
  await run.click();

  await expect(
    page.locator(".detail .pill").filter({ hasText: "completed" }),
  ).toBeVisible({ timeout: 45_000 });
  await expect(
    page.locator("pre").filter({ hasText: "E2E mock inference response" }),
  ).toBeVisible({ timeout: 45_000 });

  expect(page.url()).not.toContain(API_TOKEN);
  expect(await page.evaluate(() => localStorage.length)).toBe(0);
  await page.getByTitle("Disconnect and clear token").click();
  await expect(page.getByPlaceholder("DREAD_API_TOKEN")).toBeVisible();
});
