import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

import type {
  CodexInventory,
  CreatePackageReport,
  CreatePackageRequest,
  PackagePreview,
  RegistrationStatus,
  RestoreOptions,
  RestorePlan,
  RestoreReport,
  RollbackReport,
  TransactionSummary,
} from "./types";

export function discoverCodex(overrideHome?: string): Promise<CodexInventory> {
  return invoke("discover_codex", { overrideHome: overrideHome ?? null });
}

export function createPackage(request: CreatePackageRequest): Promise<CreatePackageReport> {
  return invoke("create_package", { request });
}

export function inspectPackage(path: string): Promise<PackagePreview> {
  return invoke("inspect_package", { path });
}

export function buildRestorePlan(
  packagePath: string,
  targetCodexHome: string,
  projectsRoot: string,
): Promise<RestorePlan> {
  return invoke("build_restore_plan", { packagePath, targetCodexHome, projectsRoot });
}

export function applyRestore(planId: string, options: RestoreOptions): Promise<RestoreReport> {
  return invoke("apply_restore", { planId, options });
}

export function listTransactions(): Promise<TransactionSummary[]> {
  return invoke("list_transactions");
}

export function rollbackTransaction(transactionId: string): Promise<RollbackReport> {
  return invoke("rollback_transaction", { transactionId });
}

export function openPath(path: string, transactionId?: string): Promise<void> {
  return invoke("open_path", { path, transactionId: transactionId ?? null });
}

export function openRestoredThread(
  path: string,
  transactionId: string,
): Promise<RegistrationStatus> {
  return invoke("open_restored_thread", { path, transactionId });
}

export async function pickRehomePackage(): Promise<string | null> {
  const path = await open({
    title: "选择 ReHome 包",
    multiple: false,
    directory: false,
    filters: [{ name: "ReHome 包", extensions: ["rehome"] }],
  });
  return typeof path === "string" ? path : null;
}

export async function pickRehomeSavePath(defaultPath?: string): Promise<string | null> {
  const path = await save({
    title: "保存 ReHome 包",
    defaultPath,
    filters: [{ name: "ReHome 包", extensions: ["rehome"] }],
  });
  if (!path) return null;
  return path.toLowerCase().endsWith(".rehome") ? path : `${path}.rehome`;
}

export async function pickDirectory(
  title: string,
  defaultPath?: string,
): Promise<string | null> {
  const path = await open({
    title,
    defaultPath,
    multiple: false,
    directory: true,
  });
  return typeof path === "string" ? path : null;
}
