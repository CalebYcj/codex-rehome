import { invoke } from "@tauri-apps/api/core";

import type {
  CodexInventory,
  CreatePackageReport,
  CreatePackageRequest,
  PackagePreview,
  RegistrationStatus,
  RestoreOptions,
  RestoreLocationSelection,
  RestorePlan,
  RestoreReport,
  RollbackAction,
  RollbackReport,
  TransactionHistory,
} from "./types";

export function discoverCodex(): Promise<CodexInventory> {
  return invoke("discover_codex");
}

export function createPackage(selection: CreatePackageRequest): Promise<CreatePackageReport | null> {
  return invoke("create_package", { selection });
}

export function inspectPackage(): Promise<PackagePreview | null> {
  return invoke("inspect_package");
}

export async function selectRestoreDestinations(
  packageSelectionId: string,
): Promise<RestoreLocationSelection | null> {
  const response = await invoke<
    | ({ action: "destinations" } & RestoreLocationSelection)
    | { action: "plan"; plan: RestorePlan }
    | null
  >("build_restore_plan", {
    request: { action: "select_destinations", package_selection_id: packageSelectionId },
  });
  return response?.action === "destinations" ? response : null;
}

export async function buildRestorePlan(
  packageSelectionId: string,
  destinationSelectionId: string,
): Promise<RestorePlan> {
  const response = await invoke<{ action: "plan"; plan: RestorePlan } | null>(
    "build_restore_plan",
    {
      request: {
        action: "build",
        package_selection_id: packageSelectionId,
        destination_selection_id: destinationSelectionId,
      },
    },
  );
  if (response?.action !== "plan") throw new Error("恢复位置选择已取消");
  return response.plan;
}

export function applyRestore(planId: string, options: RestoreOptions): Promise<RestoreReport> {
  return invoke("apply_restore", { selection: { plan_id: planId, ...options } });
}

export function listTransactions(): Promise<TransactionHistory> {
  return invoke("list_transactions");
}

export function rollbackTransaction(
  transactionId: string,
  action: RollbackAction,
): Promise<RollbackReport> {
  return invoke("rollback_transaction", {
    selection: { transaction_id: transactionId, action },
  });
}

export function openPath(pathOrObjectId: string, transactionId?: string): Promise<void> {
  const selection = transactionId
    ? { kind: "transaction", path: pathOrObjectId, transaction_id: transactionId }
    : { kind: "granted", object_id: pathOrObjectId };
  return invoke("open_path", { selection });
}

export function openRestoredThread(
  path: string,
  transactionId: string,
): Promise<RegistrationStatus> {
  return invoke("open_restored_thread", {
    selection: { path, transaction_id: transactionId },
  });
}
